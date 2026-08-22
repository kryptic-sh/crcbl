//! `crcbl-hal`'s seam, on whichever backend the registry opens.
//!
//! # Why this exists
//!
//! The same seam behaviours were covered three times over, once per backend:
//! `crcbl-vk`'s `tests/vk_e2e/`, `crcbl-wgpu`'s `tests/wgpu_e2e.rs`, and the
//! `#[ignore]`d device tests living inside `crcbl-mtl`'s and `crcbl-dx12`'s own
//! `src/`. Three hand-maintained copies of one claim is three things that drift
//! apart, and a copy that drifts is worse than a copy that is missing: it goes
//! on passing while asserting something the other backends are not held to.
//!
//! This file is the single owner of the behaviours that are the *seam's* and not
//! a backend's. It names no backend type — only `crcbl::hal` traits and
//! `crcbl::backend::open` — so one binary is the whole matrix, driven by
//! `CRCBL_GPU` exactly as `tests/render_e2e.rs` is.
//!
//! What stays behind in a backend's own suite is what is genuinely that
//! backend's: validation- or debug-layer wiring, capability tiers, adapter
//! enumeration, and refusals that exist because one API has a limit the others
//! do not.
//!
//! # The backend must be named
//!
//! Every backend implements this seam identically by construction — that is what
//! makes the suite shared — so a run that silently fell back to another backend
//! passes and proves nothing about the one that was wanted.
//! [`assert_backend_matches_the_pin`] compares the opened backend against
//! `CRCBL_GPU`, and `tests/run-hal-seam-e2e.sh` refuses to run without it. Same
//! contract, same reason, same runner conventions as `run-render-e2e.sh`.
//!
//! # What stands in for the validation report
//!
//! `crcbl-vk`'s fixture ends every test by asserting its validation report is
//! clean, and that assertion is the whole of several of those tests. There is no
//! cross-backend equivalent of a Vulkan validation layer, so [`Headless::finish`]
//! asserts on [`Device::take_error`] instead — the seam's own out-of-band error
//! channel. It is not the same instrument and this file does not pretend it is:
//! on wgpu and WebGPU it is where a failure the return value did not carry
//! actually arrives, and on Vulkan, Metal and D3D12 the trait's default answers
//! `None` because those backends report through their return values.
//!
//! So a test whose evidence is a return value or a byte moved here and its
//! copies were deleted. Two obligations are different: 1 (a device outliving its
//! instance) and 2b (a swapchain outliving its surface handle) have a
//! *functional* half every backend can be held to, which is what is asserted
//! below, and a **use-after-free-inside-the-driver** half that only a validation
//! layer can witness. `crcbl-vk`'s `vk_e2e/seam_obligations.rs` keeps that
//! second half and says so; it is not a duplicate of these two, it is the part
//! of the claim this file cannot make.

#![cfg(feature = "hal-seam-e2e")]

use core::ops::Deref;
use std::time::{Duration, Instant};

use crcbl::adapter::ADAPTER_ENV_VAR;
use crcbl::backend::{BACKEND_ENV_VAR, GpuBackend};
use crcbl::core::SurfaceTarget;
use crcbl::hal::{
    Barriers, BufferDesc, BufferImageCopy, BufferUsage, ClearValue, ColorAttachment,
    CommandEncoderDesc, CompositeAlpha, DepthStencilAttachment, Device, DeviceDesc, DrawIndirect,
    Extent3d, Features, Format, HalError, ImageAspect, ImageBarrier, ImageCopy, ImageDesc,
    ImageSubresourceLayers, ImageSubresourceRange, ImageType, ImageUsage, ImageViewDesc,
    ImageViewType, Instance, LoadOp, MemoryLocation, Offset3d, PolygonMode, PresentInfo,
    PresentMode, PrimitiveState, QueryKind, QuerySetDesc, QuerySetHandle, ReadbackDesc,
    ReadbackState, Rect2d, RenderPassDesc, ResourceState, SamplerDesc, StoreOp, SubmitInfo,
    SurfaceError, SwapchainDesc,
};

/// The size every offscreen test in this file renders at.
///
/// `crcbl-vk`'s `vk_e2e` fixture's own extent, kept rather than rounded: small
/// enough that a software rasteriser is fast, and neither dimension is a
/// multiple of 256, so a readback whose rows a backend padded to its own
/// alignment cannot be mistaken for a tightly packed one.
const EXTENT: (u32, u32) = (64, 48);

/// A distinctive clear colour. Chosen so every channel differs and none is 0 or
/// 1: a channel-swap or an sRGB round-trip bug is then visible in the bytes.
const CLEAR: [f32; 4] = [0.25, 0.5, 0.75, 1.0];

/// The byte every readback destination is filled with before it is polled.
///
/// Deliberately not `0`. A frame that legitimately rendered black reads back as
/// zeroes, and so does a destination no copy ever reached — an ambiguity that
/// makes the failing assertion unable to say whether the frame was empty or the
/// readback never landed. This value is neither a plausible pixel nor a
/// plausible counter, so a byte of it surviving into an assertion is evidence
/// that nothing was copied over it.
const POISON: u8 = 0xA5;

/// The optional features `CRCBL_SEAM_WITHHOLD` asks this run not to request.
///
/// **This exists to make the refusal half of the parity contract run.** See
/// [`Headless::open_with_every_optional_feature`], which is its only caller and
/// where the reasoning lives.
///
/// The only accepted value is `all`, which asks for no optional feature at all.
/// A list of individual names would need this file to carry a name-to-flag
/// table, which is knowledge `crcbl-hal` already owns and this crate cannot
/// reach — `bitflags` is not among its dependencies and one flag parser is not
/// worth becoming one. `all` is also the value that does the job: every
/// capability a backend gates on a feature moves to the refusal side at once.
///
/// Anything else is a panic rather than a silent empty set, because a value
/// that quietly withheld nothing would report the wide run's answers under this
/// run's name.
fn withheld_features() -> Features {
    match std::env::var(WITHHOLD_ENV_VAR) {
        Err(_) => Features::empty(),
        Ok(value) if value.eq_ignore_ascii_case("all") => Features::all(),
        Ok(value) => panic!(
            "{WITHHOLD_ENV_VAR}={value} is not a value this suite accepts; the only one is `all`. \
             Withholding nothing would report the wide run's answers under this run's name, so \
             this refuses instead."
        ),
    }
}

/// The variable [`withheld_features`] reads.
const WITHHOLD_ENV_VAR: &str = "CRCBL_SEAM_WITHHOLD";

/// The optional features [`Headless::open`] asks for.
///
/// The set a renderer actually wants, so the fixture exercises the device a
/// frame would get. [`Headless::open_with_every_optional_feature`] is the one
/// exception and says why it needs a wider ask.
const DEFAULT_OPTIONAL: Features = Features::GPU_DRIVEN.union(Features::TIMESTAMP_QUERY);

/// A readback destination of `len` bytes, filled with [`POISON`].
fn poisoned(len: usize) -> Vec<u8> {
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
/// The one failure this suite cannot survive silently: every test here passes on
/// every backend by design, so a fallback produces a green run that is evidence
/// about a backend nobody named. Both names go through
/// [`GpuBackend::from_name`] rather than a second table.
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
/// The line is load-bearing outside this file: `run-hal-seam-e2e.sh` greps it to
/// report which device really ran and to compare the pin it exported against the
/// one that arrived, and it fails when the suite never printed one. A variable
/// that never reached the test process and a pin that was honoured look
/// identical from inside — in both cases [`crcbl::adapter::pin`] answers `None`
/// and the first adapter is taken.
fn select_adapter(instance: &dyn Instance) -> crcbl::hal::AdapterInfo {
    let adapters = instance.adapters();
    let pin = crcbl::adapter::pin();
    let adapter = crcbl::adapter::select(pin.as_deref(), &adapters)
        .unwrap_or_else(|miss| panic!("{miss}"))
        .clone();
    eprintln!(
        "crcbl hal seam e2e: device on adapter {id} {name:?} type={kind:?} ({ADAPTER_ENV_VAR}={pin})",
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
/// every `headless.device.…` reading as it did in the suites this came from.
struct DeviceSlot(Option<Box<dyn Device>>);

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
struct Headless {
    instance: Box<dyn Instance>,
    device: DeviceSlot,
    surface: crcbl::hal::SurfaceHandle,
    swapchain: crcbl::hal::SwapchainHandle,
    queue: crcbl::hal::QueueHandle,
    format: Format,
}

impl Headless {
    fn open() -> Self {
        Self::open_with(EXTENT, 2)
    }

    fn open_with(extent: (u32, u32), image_count: u32) -> Self {
        Self::open_full(extent, image_count, DEFAULT_OPTIONAL)
    }

    /// The fixture with **every** optional feature asked for, minus whatever
    /// `CRCBL_SEAM_WITHHOLD` names.
    ///
    /// What the capability tests open, and the distinction matters there and
    /// nowhere else: a device is granted the intersection of what the adapter
    /// has and what the caller *asked for*, so a feature nobody requested reads
    /// back identically to one the adapter lacks. Every other test in this file
    /// wants the narrow ask, because it is the ask a renderer makes; the parity
    /// report wants the wide one, or it reports this fixture's shopping list as
    /// though it were the hardware's limits.
    ///
    /// # Why the ask can be narrowed on purpose
    ///
    /// **The parity contract has two directions and only one of them runs on a
    /// capable device.** On an RX 7900 XTX with everything asked for, `crcbl-vk`
    /// declares all 24 capabilities supported — so "declared supported must
    /// work" is exercised 24 times and "declared unsupported must refuse" is
    /// exercised **never**. Thirteen of those capabilities are gated on a
    /// device feature, so withholding the features puts them on the other side
    /// of the table and makes the refusal arm run on a real backend.
    ///
    /// Set `CRCBL_SEAM_WITHHOLD=all` to ask for no optional feature at all:
    ///
    /// ```text
    /// CRCBL_SEAM_WITHHOLD=all CRCBL_GPU=vk crates/crcbl/tests/run-hal-seam-e2e.sh
    /// ```
    fn open_with_every_optional_feature() -> Self {
        Self::open_full(EXTENT, 2, Features::all().difference(withheld_features()))
    }

    fn open_full(extent: (u32, u32), image_count: u32, optional_features: Features) -> Self {
        let instance = instance();
        let adapter = select_adapter(instance.as_ref());

        let target = SurfaceTarget::Offscreen;
        // SAFETY: `Offscreen` names no platform object at all, so there is
        // nothing to outlive the surface. `finish` tears the swapchain down
        // before the surface regardless, which is the general rule.
        let surface = unsafe { instance.create_surface(&target) }.expect("offscreen always works");

        let caps = instance
            .surface_caps(surface, adapter.id)
            .expect("the offscreen ring reports its own caps");
        let format = caps.preferred_format().expect("some format is offered");

        let device = instance
            .create_device(&DeviceDesc {
                label: Some("crcbl hal seam e2e"),
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

        let swapchain = device
            .create_swapchain(&SwapchainDesc {
                label: Some("crcbl hal seam e2e ring"),
                surface,
                format,
                extent,
                image_count,
                present_mode: PresentMode::Fifo,
                composite_alpha: CompositeAlpha::Opaque,
            })
            .expect("the ring is created");

        Self {
            instance,
            device: DeviceSlot::new(device),
            surface,
            swapchain,
            queue,
            format,
        }
    }

    /// Reads `size` bytes of `staging` back into `out`, polling with a deadline
    /// rather than sleeping — `docs/plan/12-testing.md`.
    fn readback(&self, staging: crcbl::hal::BufferHandle, size: u64, out: &mut [u8]) {
        let device = self.device.as_ref();
        let readback = device
            .request_readback(&ReadbackDesc {
                label: Some("crcbl hal seam e2e readback"),
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
    fn finish(mut self) {
        self.device.wait_idle().expect("idle");
        self.device.destroy_swapchain(self.swapchain);
        self.instance.destroy_surface(self.surface);
        assert_no_out_of_band_error(self.device.as_ref());
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
        eprintln!(
            "crcbl hal seam e2e: the fixture was dropped by a panicking test, so `finish` \
             never ran. What it can still see:"
        );
        match self.device.0.as_ref() {
            // The distinction a flake needs: a lost device is a driver-side
            // failure that makes every other symptom downstream noise, and it is
            // otherwise invisible because nothing in a failing test asks.
            Some(device) => {
                match device.wait_idle() {
                    Ok(()) => eprintln!(
                        "crcbl hal seam e2e:   wait_idle: Ok — the device is alive and idle, \
                         so the submission completed and the failure is in what it produced"
                    ),
                    Err(HalError::DeviceLost(detail)) => eprintln!(
                        "crcbl hal seam e2e:   wait_idle: device lost ({detail}) — nothing \
                         this test read back means anything"
                    ),
                    Err(error) => eprintln!("crcbl hal seam e2e:   wait_idle: {error}"),
                }
                match device.take_error() {
                    Some(error) => {
                        eprintln!("crcbl hal seam e2e:   out-of-band error: {error}")
                    }
                    None => eprintln!("crcbl hal seam e2e:   out-of-band error: none"),
                }
            }
            None => eprintln!(
                "crcbl hal seam e2e:   wait_idle: not asked, `finish` already destroyed the \
                 device"
            ),
        }
    }
}

/// Fails on anything the device reported through [`Device::take_error`].
///
/// See this file's header for what this is and is not: on wgpu and WebGPU it is
/// where a shader that would not compile or a command buffer the implementation
/// discarded actually surfaces, and on the backends that report every failure
/// through their return values the trait's default answers `None` and this is
/// vacuous.
fn assert_no_out_of_band_error(device: &dyn Device) {
    if let Some(error) = device.take_error() {
        panic!(
            "the device reported out of band: {error}. Every call in this test returned \
             success, so this is a failure the return values did not carry."
        );
    }
}

/// Opens a headless device on `instance` with nothing required.
///
/// [`DeviceDesc::for_adapter`] requires compute and a timeline semaphore, which
/// a device may not have — but that is the *only* reason it may fail here, and
/// swallowing any error would let a genuine one hide behind the fallback.
fn open_headless_device(
    instance: &dyn Instance,
    adapter: crcbl::hal::AdapterId,
) -> Box<dyn Device> {
    match instance.create_device(&DeviceDesc::for_adapter(adapter)) {
        Ok(device) => device,
        Err(error) => {
            assert!(
                matches!(error, HalError::UnsupportedFeatures { .. }),
                "the only permitted refusal of the headless default is a named feature gap: \
                 {error}"
            );
            instance
                .create_device(&DeviceDesc {
                    label: None,
                    adapter,
                    required_features: Features::empty(),
                    optional_features: Features::empty(),
                    compatible_surface: None,
                })
                .expect("a featureless headless device opens on any adapter")
        }
    }
}

// --- the seam's own obligations --------------------------------------------

/// **Obligation 4**: a zero extent is the caller's problem, and the error says
/// so rather than the backend guessing a size.
///
/// Migrated from `crcbl-vk`'s `vk_e2e/seam_obligations.rs` and `crcbl-wgpu`'s
/// `wgpu_e2e.rs`, which each asserted it against their own backend. The wgpu
/// copy's *three* extents are the ones kept: a window minimised in one axis is
/// the shape a tiling manager actually produces, and a backend that guarded only
/// `width == 0 && height == 0` passes the vk copy and fails here.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-hal-seam-e2e.sh"]
fn a_zero_extent_swapchain_is_refused_with_a_reason() {
    let instance = instance();
    let adapter = select_adapter(instance.as_ref());
    // SAFETY: `Offscreen` names no platform object.
    let surface = unsafe { instance.create_surface(&SurfaceTarget::Offscreen) }.expect("offscreen");
    let device = instance
        .create_device(&DeviceDesc {
            label: Some("crcbl hal seam e2e"),
            adapter: adapter.id,
            required_features: Features::empty(),
            optional_features: Features::empty(),
            compatible_surface: Some(surface),
        })
        .expect("a device opens");

    for extent in [(0, 48), (64, 0), (0, 0)] {
        let error = device
            .create_swapchain(&SwapchainDesc {
                label: None,
                surface,
                format: Format::Rgba8UnormSrgb,
                extent,
                image_count: 2,
                present_mode: PresentMode::Fifo,
                composite_alpha: CompositeAlpha::Opaque,
            })
            .err()
            .unwrap_or_else(|| {
                panic!("a swapchain at {extent:?} means 'not yet', not 'guess a size'")
            });
        let SurfaceError::Hal(HalError::InvalidDescriptor(message)) = error else {
            panic!("{extent:?} was refused as the wrong kind of error: {error}");
        };
        assert!(message.contains("do not create one yet"), "{message}");
    }

    instance.destroy_surface(surface);
    assert_no_out_of_band_error(device.as_ref());
    drop(device);
}

/// **Obligation 3**: handles do not cross devices, and the failure is detected
/// rather than undefined.
///
/// Migrated from `crcbl-vk`'s `vk_e2e/seam_obligations.rs` and `crcbl-wgpu`'s
/// `wgpu_e2e.rs`, which had the same test twice; `crcbl-mtl` and `crcbl-dx12`
/// each had a third and fourth copy in their own `src/`. The wgpu copy is the
/// fuller one and is the shape kept: it occupies the second device's slot first,
/// asserts the two generations are equal, checks the foreign *destroy* does not
/// take the local object, and covers the queue as well as the buffer.
///
/// `ForeignObject` specifically, not "either error will do": every object table
/// is per-device, so this used to be answered by the handle happening to miss
/// the second device's pool — which stops being true the moment two devices
/// allocate in step, and at that point device A accepted device B's handle
/// outright. A handle carries the tag of the device that issued it, so the
/// answer is decided rather than lucky.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-hal-seam-e2e.sh"]
fn a_handle_from_one_device_is_foreign_to_another() {
    let instance = instance();
    let adapter = select_adapter(instance.as_ref());
    let first = open_headless_device(instance.as_ref(), adapter.id);
    let second = open_headless_device(instance.as_ref(), adapter.id);

    let describe = |label| BufferDesc {
        label: Some(label),
        size: 256,
        usage: BufferUsage::TRANSFER_SRC,
        memory: MemoryLocation::HostUpload,
    };
    let on_first = first
        .create_buffer(&describe("on the first"))
        .expect("a buffer");
    let on_second = second
        .create_buffer(&describe("on the second"))
        .expect("a buffer occupying the slot the first device's handle would land in");
    assert_eq!(
        on_first.generation(),
        on_second.generation(),
        "both pools are fresh, so only the tag can tell these apart"
    );

    let error = second
        .write_buffer(on_first, 0, &[1, 2, 3, 4])
        .expect_err("a buffer from another device must be refused");
    assert!(
        matches!(error, HalError::ForeignObject { kind: "buffer", .. }),
        "a live handle from another device is foreign, not merely unresolvable: {error}"
    );

    // The second device's own still resolves, so the check is not simply
    // refusing everything.
    second
        .write_buffer(on_second, 0, &[1, 2, 3, 4])
        .expect("its own buffer resolves");

    // A destroy with a foreign handle must not take the local object that shares
    // its bits.
    second.destroy_buffer(on_first);
    second
        .write_buffer(on_second, 0, &[0xEE; 4])
        .expect("its buffer survived a foreign destroy");

    // The queue is synthesised rather than pooled, and obligation 3 covers it
    // too: without the tag, every device accepted every other device's.
    let queue_of_first = first
        .queue(crcbl::hal::QueueKind::Graphics)
        .expect("a graphics queue always exists");
    let error = second
        .submit(queue_of_first, &SubmitInfo::new(&[]))
        .expect_err("the first device's queue is not the second's to submit to");
    assert!(
        matches!(error, HalError::ForeignObject { kind: "queue", .. }),
        "{error}"
    );

    first.destroy_buffer(on_first);
    second.destroy_buffer(on_second);
    assert_no_out_of_band_error(first.as_ref());
    assert_no_out_of_band_error(second.as_ref());
    drop(second);
    drop(first);
}

/// **Obligation 3, across two instances.** Handle bits are only unique within
/// the backend that issued them, so two instances genuinely hand out identical
/// ones — a surface crossing them must be detected, and detected as
/// `ForeignObject` rather than as a stale handle, because the two send a reader
/// to different bugs.
///
/// Migrated from `crcbl-vk`'s `vk_e2e/seam_obligations.rs`.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-hal-seam-e2e.sh"]
fn a_surface_from_one_instance_is_foreign_to_another() {
    let owner = instance();
    let other = instance();

    // SAFETY: `Offscreen` names no platform object at all.
    let surface =
        unsafe { owner.create_surface(&SurfaceTarget::Offscreen) }.expect("offscreen always works");
    let adapter = select_adapter(other.as_ref());

    let error = other
        .surface_caps(surface, adapter.id)
        .expect_err("this surface belongs to the other instance");
    assert!(
        matches!(
            error,
            HalError::ForeignObject {
                kind: "surface",
                ..
            }
        ),
        "a live handle from another instance is foreign, not unresolvable: {error}"
    );

    let error = other
        .create_device(&DeviceDesc {
            label: Some("foreign surface"),
            adapter: adapter.id,
            required_features: Features::empty(),
            optional_features: Features::empty(),
            compatible_surface: Some(surface),
        })
        .expect_err("and the same on the device-creation path");
    assert!(
        matches!(
            error,
            HalError::ForeignObject {
                kind: "surface",
                ..
            }
        ),
        "{error}"
    );

    // The other instance must not be able to free it either, which is what stops
    // a stray `destroy_surface` from turning a bit collision into a double free.
    other.destroy_surface(surface);
    owner
        .surface_caps(surface, select_adapter(owner.as_ref()).id)
        .expect("the owning instance still has its surface");

    owner.destroy_surface(surface);
}

/// **Obligation 1**: a `Device` may outlive its `Instance`.
///
/// Migrated from `crcbl-vk`'s `vk_e2e/seam_obligations.rs`, where the assertion
/// was the validation layer's report — the class of bug this catches is a
/// use-after-free inside the driver. There is no cross-backend layer to ask, so
/// what this asserts instead is that the device goes on *working* with its
/// instance gone: a buffer created, written, destroyed and the queue driven to
/// idle, all after the drop.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-hal-seam-e2e.sh"]
fn a_device_outlives_the_instance_that_made_it() {
    let instance = instance();
    let adapter = select_adapter(instance.as_ref());
    let device = instance
        .create_device(&DeviceDesc {
            label: Some("outlives"),
            adapter: adapter.id,
            required_features: Features::empty(),
            optional_features: Features::empty(),
            compatible_surface: None,
        })
        .expect("a headless device opens");

    drop(instance);

    let buffer = device
        .create_buffer(&BufferDesc {
            label: Some("after the instance went away"),
            size: 64,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostUpload,
        })
        .expect("the device outlived its instance");
    device.write_buffer(buffer, 0, &[7; 64]).expect("write");
    device.destroy_buffer(buffer);
    device.wait_idle().expect("idle");
    assert_no_out_of_band_error(device.as_ref());
    drop(device);
}

/// **Obligation 2b**: `destroy_surface` invalidates the handle *immediately*,
/// and everything already built on that surface goes on working.
///
/// Migrated from `crcbl-vk`'s `vk_e2e/seam_obligations.rs`. The two halves fail
/// for different reasons and are asserted separately: the handle is *stale*
/// rather than foreign — the two send a reader to different bugs — and a whole
/// frame still renders and presents through the swapchain built on it.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-hal-seam-e2e.sh"]
fn a_swapchain_keeps_working_after_its_surface_handle_is_destroyed() {
    let mut headless = Headless::open();

    // The handle dies here, mid-life of the swapchain built on it.
    headless.instance.destroy_surface(headless.surface);
    let error = headless
        .instance
        .surface_caps(headless.surface, crcbl::hal::AdapterId(0))
        .expect_err("the handle is invalid the moment the caller destroys it");
    assert!(
        matches!(error, HalError::InvalidHandle { .. }),
        "a destroyed surface handle is stale, not foreign: {error}"
    );

    // And the swapchain on it still renders a whole frame.
    let device = &headless.device;
    let acquired = device
        .acquire_next_frame(headless.swapchain)
        .expect("the ring outlives the surface handle");
    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("after the surface handle went away"),
        queue: headless.queue,
    });
    encoder.pipeline_barrier(&Barriers {
        images: &[ImageBarrier::new(
            acquired.image,
            ImageSubresourceRange::all(headless.format),
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
    let commands = encoder.finish().expect("recording succeeded");
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");
    device
        .present(
            headless.queue,
            &PresentInfo {
                swapchain: headless.swapchain,
                waits: acquired.present_semaphore.as_slice(),
                present_id: None,
            },
        )
        .expect("present");
    device.wait_idle().expect("idle");
    device.destroy_command_buffer(commands);

    // Teardown in the order the deferral is for: the swapchain last, long after
    // the surface handle. `Headless::finish` cannot be used because it destroys
    // the surface, which this test already did.
    device.destroy_swapchain(headless.swapchain);
    assert_no_out_of_band_error(headless.device.as_ref());
    headless.device.destroy();
}

/// **The surface offers an sRGB format, and that is what
/// [`crcbl::hal::SurfaceCaps::preferred_format`] hands back.**
///
/// The whole colour pipeline rests on this and nothing used to check it. Every
/// pass above the seam writes display-referred values and leaves the encode to
/// the hardware, so a swapchain configured with a linear format presents a
/// transfer function too dark — and no call fails, no error is reported, and no
/// existing gate looks: the frame is a perfectly valid render into a perfectly
/// valid target, it is just the wrong picture. That is exactly how it shipped to
/// a user in the browser build, where `surface_caps` answered the canvas's
/// linear formats and `preferred_format` fell through to the first of them.
///
/// [`crcbl::hal::SurfaceCaps::formats`] now states the obligation rather than
/// leaving it to each backend's own unit tests, and this is where every backend
/// is held to it. The fallback in `preferred_format` — the first format at all,
/// when no sRGB one is offered — is a total function's last arm and not a
/// configuration the engine can run in, which is why reaching it is a failure
/// here rather than a shrug.
///
/// # What this does and does not reach
///
/// A **[`SurfaceTarget::Offscreen`] surface**, because this suite runs on
/// whichever backend `CRCBL_GPU` names and offscreen is the one surface every
/// backend can create anywhere. So on `vk`, `dx12` and `mtl` it pins each
/// backend's own offscreen table (`crcbl-vk`'s `offscreen_surface_caps`,
/// `crcbl-dx12`'s `OFFSCREEN_FORMATS`, `crcbl-mtl`'s offscreen arm) and not what
/// a real window system answered. That is a weaker claim than it looks and it is
/// stated rather than glossed.
///
/// What covers the other half is no longer nothing: `tests/windowed_e2e.rs`,
/// behind the `windowed-e2e` feature, puts a real `crcbl-vk` swapchain on a real
/// X11 window and holds it to what the server actually reports — the extent
/// clamp in particular, which no fabricated `SurfaceCaps` can make a claim
/// about. That suite is Vulkan-and-X11 only, so `dx12` and `mtl` are still
/// covered on the windowed path by their own unit tests over a fabricated
/// `SurfaceCaps` and by nothing else. `crcbl-webgpu`'s canvas — the one that
/// actually regressed — is covered by `crcbl-webgpu`'s `hal/tests.rs` and by
/// group I of the standalone probe, neither of which this binary can run.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-hal-seam-e2e.sh"]
fn a_surface_offers_an_srgb_format_and_preferred_format_picks_it() {
    let instance = instance();
    let adapter = select_adapter(instance.as_ref());
    // SAFETY: `Offscreen` names no platform object at all.
    let surface = unsafe { instance.create_surface(&SurfaceTarget::Offscreen) }
        .expect("offscreen always works");

    let caps = instance
        .surface_caps(surface, adapter.id)
        .expect("a surface reports its caps");

    assert!(
        !caps.formats.is_empty(),
        "a surface that offers no format at all is a backend that should have failed \
         earlier, per `SurfaceCaps::preferred_format`"
    );
    assert!(
        caps.formats.iter().any(|format| format.is_srgb()),
        "no sRGB format among {:?}. Every pass above the seam writes display-referred \
         values and leaves the encode to the hardware, so a linear swapchain presents a \
         transfer function too dark and nothing else in the frame's path says so.",
        caps.formats
    );

    let preferred = caps
        .preferred_format()
        .expect("a non-empty format list has a preference");
    assert!(
        preferred.is_srgb(),
        "preferred_format() answered {preferred:?}, which is linear, out of {:?}. It takes \
         the first sRGB entry and falls through to the first entry of all, so this is the \
         fallback firing — and the fallback is a total function's last arm, not a mode the \
         engine renders correctly in.",
        caps.formats
    );
    eprintln!(
        "crcbl hal seam e2e: the surface prefers {preferred:?} out of {:?}",
        caps.formats
    );

    instance.destroy_surface(surface);
}

// --- a frame's worth of the seam -------------------------------------------

/// The sRGB transfer function, encoding linear light into a display format's
/// levels.
///
/// The standard piecewise curve — IEC 61966-2-1, which is what Vulkan's
/// `*_SRGB` formats, Metal's `*_sRGB` ones and D3D12's `*_SRGB` ones are each
/// defined to apply on a write. Transcribed rather than reached for because
/// this workspace has no shared home for it: `tests/render_e2e.rs`,
/// `crcbl-vk`'s `vk_e2e/sprite` and `vk_e2e/depth_probe`, and
/// `crcbl-scene`'s `gltf_render` each carry their own, and a test binary
/// cannot borrow another test binary's. The constants are the specification's,
/// and [`a_render_pass_clear_reaches_memory_with_the_colour_it_was_given`] is
/// what checks the transcription: it asserts against a value the hardware
/// produced independently.
fn srgb_encode(linear: f32) -> f32 {
    if linear <= 0.003_130_8 {
        12.92 * linear
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

/// The byte a channel of `CLEAR` must arrive as, given the target's format.
///
/// Both arms are wanted, and they are the two halves of the failure this test
/// exists for: an sRGB target encodes on write and a linear one does not, so
/// the *same* clear lands as two very different byte triples and each is the
/// other's evidence of a bug.
fn expected_level(linear: f32, format: Format) -> u8 {
    let value = if format.is_srgb() {
        srgb_encode(linear)
    } else {
        linear
    };
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        (value * 255.0).round().clamp(0.0, 255.0) as u8
    }
}

/// How far a channel may sit from [`expected_level`].
///
/// One level, for 8-bit rounding and for the encode's own precision — the
/// specification fixes the curve but not the implementation's arithmetic. It is
/// nowhere near wide enough to swallow the failure this guards: [`CLEAR`]'s
/// channels encode to `137, 188, 225` and land at `64, 128, 191` unencoded,
/// which is 34 to 73 levels away.
const LEVEL_TOLERANCE: u8 = 1;

/// A render-pass clear that really reached host memory, in the colour it was
/// given and in the channel order the format names.
///
/// Migrated from `crcbl-vk`'s `vk_e2e/swapchain.rs`. A clear command would put
/// the same bytes there while exercising none of the attachment, load-op or
/// layout machinery every later milestone is built on — so this goes through
/// [`crcbl::hal::CommandEncoder::begin_render_pass`] with [`LoadOp::Clear`],
/// which is what "clear colour through the graph" means.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-hal-seam-e2e.sh"]
fn a_render_pass_clear_reaches_memory_with_the_colour_it_was_given() {
    let headless = Headless::open();
    let device = &headless.device;

    let acquired = device
        .acquire_next_frame(headless.swapchain)
        .expect("the ring always has an image");

    // The swapchain hands over its own view and the extent it was configured at.
    // Neither is derived here — that is the whole point of the two fields.
    assert_eq!(
        acquired.extent, EXTENT,
        "an offscreen ring has no window system to clamp against, so the configured extent \
         is the requested one"
    );

    let pixels = u64::from(EXTENT.0) * u64::from(EXTENT.1) * 4;
    let staging = device
        .create_buffer(&BufferDesc {
            label: Some("crcbl hal seam e2e readback"),
            size: pixels,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a readback buffer");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("crcbl hal seam e2e frame"),
        queue: headless.queue,
    });
    encoder.pipeline_barrier(&Barriers {
        images: &[ImageBarrier::new(
            acquired.image,
            ImageSubresourceRange::all(headless.format),
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
            ImageSubresourceRange::all(headless.format),
            ResourceState::ColorAttachment,
            ResourceState::TransferSrc,
        )],
        ..Barriers::default()
    });
    encoder.copy_image_to_buffer(&BufferImageCopy {
        buffer: staging,
        buffer_offset: 0,
        buffer_row_length: 0,
        buffer_image_height: 0,
        image: acquired.image,
        image_subresource: ImageSubresourceLayers {
            aspect: ImageAspect::COLOR,
            mip: 0,
            base_layer: 0,
            layer_count: 1,
        },
        image_offset: Offset3d::default(),
        image_extent: Extent3d::d2(EXTENT.0, EXTENT.1),
    });
    let commands = encoder.finish().expect("recording succeeded");

    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");
    device
        .present(
            headless.queue,
            &PresentInfo {
                swapchain: headless.swapchain,
                waits: acquired.present_semaphore.as_slice(),
                present_id: None,
            },
        )
        .expect("present");

    let mut bytes = poisoned(pixels as usize);
    headless.readback(staging, pixels, &mut bytes);

    // The whole attachment really was cleared, not just part of it.
    let first: [u8; 4] = bytes[0..4].try_into().expect("four bytes");
    assert!(
        bytes.chunks_exact(4).all(|pixel| pixel == first),
        "the whole render area must be cleared uniformly; got {first:?} then {:?}",
        &bytes[4..8]
    );
    assert_ne!(first, [0, 0, 0, 0], "an all-zero result means nothing ran");
    assert_eq!(first[3], 255, "alpha 1.0 must survive");

    // The channel order in memory follows the format, which is the point of
    // checking it: a backend that ignored it would pass a "not all zero"
    // assertion and produce a blue window.
    let (r, g, b) = match headless.format {
        Format::Bgra8Unorm | Format::Bgra8UnormSrgb => (first[2], first[1], first[0]),
        _ => (first[0], first[1], first[2]),
    };
    assert!(
        r < g && g < b,
        "the clear was {CLEAR:?}, so red < green < blue must survive into memory; got r={r} \
         g={g} b={b} in {:?}",
        headless.format
    );

    // **THE ENCODE HAPPENED, AND THESE ARE THE LEVELS IT PRODUCED.** The
    // ordering above is what this test used to stop at, and it is not enough:
    // `CLEAR` satisfies red < green < blue both encoded and unencoded, so a
    // target that skipped the sRGB encode passed it unchanged. That is the exact
    // shape of the bug the browser build shipped — a swapchain image viewed in
    // the linear counterpart of its own format, so every pass wrote
    // display-referred values that nothing ever encoded — and the only assertion
    // that can tell the two apart is one on the *values*.
    //
    // `CLEAR` is why this is checkable at all: 0 and 1 are fixed points of the
    // transfer function, so the red the seam suites used to clear to reads back
    // identically either way and proves nothing about the encode.
    let want = [
        expected_level(CLEAR[0], headless.format),
        expected_level(CLEAR[1], headless.format),
        expected_level(CLEAR[2], headless.format),
        expected_level(CLEAR[3], headless.format),
    ];
    let (want_r, want_g, want_b) = match headless.format {
        Format::Bgra8Unorm | Format::Bgra8UnormSrgb => (want[2], want[1], want[0]),
        _ => (want[0], want[1], want[2]),
    };
    let off_by = |actual: u8, expected: u8| actual.abs_diff(expected) > LEVEL_TOLERANCE;
    assert!(
        !(off_by(r, want_r) || off_by(g, want_g) || off_by(b, want_b)),
        "the clear {CLEAR:?} reached memory as r={r} g={g} b={b}, and a {:?} target must put \
         r={want_r} g={want_g} b={want_b} there (±{LEVEL_TOLERANCE}).\n  \
         encoded, which is what an sRGB target owes: r={enc_r} g={enc_g} b={enc_b}\n  \
         unencoded, which is what the linear values are: r={lin_r} g={lin_g} b={lin_b}\n  \
         Landing on the unencoded row means the write skipped the sRGB encode — the target \
         was reinterpreted in the linear counterpart of its format somewhere between \
         `create_swapchain` and the attachment — and every frame this backend presents is a \
         transfer function too dark.",
        headless.format,
        enc_r = expected_level(CLEAR[0], Format::Rgba8UnormSrgb),
        enc_g = expected_level(CLEAR[1], Format::Rgba8UnormSrgb),
        enc_b = expected_level(CLEAR[2], Format::Rgba8UnormSrgb),
        lin_r = expected_level(CLEAR[0], Format::Rgba8Unorm),
        lin_g = expected_level(CLEAR[1], Format::Rgba8Unorm),
        lin_b = expected_level(CLEAR[2], Format::Rgba8Unorm),
    );
    eprintln!(
        "crcbl hal seam e2e: the clear reached memory as {first:?} in {:?}, against {want:?}",
        headless.format
    );

    device.destroy_command_buffer(commands);
    device.destroy_buffer(staging);
    headless.finish();
}

// --- the compute path ------------------------------------------------------

/// Workgroups the probe's buffers are sized for.
///
/// Eight, so `dispatch_indirect` can ask for two and leave six workgroups' worth
/// of untouched sentinel behind it — which is what tells "the argument buffer
/// was read" apart from "everything was dispatched anyway".
const PROBE_GROUPS: u32 = 8;

/// Elements the probe transforms.
const PROBE_ELEMENTS: u32 = PROBE_GROUPS * crcbl::shaders::compute_probe::WORKGROUP_SIZE;

/// What the destination buffer holds before every dispatch.
///
/// Deliberately not zero, and deliberately not a square: a destination that was
/// never written must not be confusable with one the shader wrote, and zero is
/// both its own square and what fresh device memory tends to be.
const PROBE_SENTINEL: u32 = 0xDEAD_BEEF;

/// The probe's input, one distinct value per index.
///
/// Distinct matters: with a constant input, a shader that indexed `source`
/// wrongly would still produce the right number in every slot. `index + 1`
/// avoids zero, whose square is itself.
fn probe_source() -> Vec<u32> {
    (0..PROBE_ELEMENTS).map(|index| index + 1).collect()
}

/// What the destination must hold for `elements` dispatched elements, and the
/// sentinel beyond them.
///
/// Written out here rather than derived from the shader: squaring is a closed
/// form the test states for itself, which is the whole reason the probe squares.
fn probe_expected(elements: u32) -> Vec<u32> {
    probe_source()
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            if (index as u32) < elements {
                value * value
            } else {
                PROBE_SENTINEL
            }
        })
        .collect()
}

/// Bytes one probe buffer occupies.
const fn probe_bytes() -> u64 {
    PROBE_ELEMENTS as u64 * 4
}

/// Everything one compute dispatch needs, built through the seam.
struct ComputeProbe {
    params: crcbl::hal::BufferHandle,
    source: crcbl::hal::BufferHandle,
    destination: crcbl::hal::BufferHandle,
    /// Host-readable copy target, so the result can be asserted rather than
    /// assumed.
    staging: crcbl::hal::BufferHandle,
    /// A host buffer holding nothing but [`PROBE_SENTINEL`], copied over the
    /// destination before each run. See [`ComputeProbe::run`] for why this is a
    /// copy and not a fill.
    sentinel: crcbl::hal::BufferHandle,
    bind_group_layout: crcbl::hal::BindGroupLayoutHandle,
    bind_group: crcbl::hal::BindGroupHandle,
    pipeline_layout: crcbl::hal::PipelineLayoutHandle,
    pipeline: crcbl::hal::ComputePipelineHandle,
}

impl ComputeProbe {
    /// Builds the pipeline and stages the input in.
    fn new(headless: &Headless) -> Self {
        let device = headless.device.as_ref();
        let params = crcbl::shaders::compute_probe::Params {
            count: PROBE_ELEMENTS,
        }
        .to_bytes();
        let source_bytes: Vec<u8> = probe_source()
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();

        let upload = device
            .create_buffer(&BufferDesc {
                label: Some("compute probe upload"),
                size: (params.len() + source_bytes.len()) as u64,
                usage: BufferUsage::TRANSFER_SRC,
                memory: MemoryLocation::HostUpload,
            })
            .expect("a staging buffer");
        device.write_buffer(upload, 0, &params).expect("write");
        device
            .write_buffer(upload, params.len() as u64, &source_bytes)
            .expect("write");

        let params_buffer = device
            .create_buffer(&BufferDesc {
                label: Some("compute probe params"),
                size: crcbl::shaders::compute_probe::PARAMS_SIZE as u64,
                usage: BufferUsage::UNIFORM | BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("a uniform buffer");
        let source = device
            .create_buffer(&BufferDesc {
                label: Some("compute probe source"),
                size: probe_bytes(),
                usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("a source buffer");
        let destination = device
            .create_buffer(&BufferDesc {
                label: Some("compute probe destination"),
                size: probe_bytes(),
                usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_DST | BufferUsage::TRANSFER_SRC,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("a destination buffer");
        let staging = device
            .create_buffer(&BufferDesc {
                label: Some("compute probe readback"),
                size: probe_bytes(),
                usage: BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::HostReadback,
            })
            .expect("a readback buffer");
        let sentinel = device
            .create_buffer(&BufferDesc {
                label: Some("compute probe sentinel"),
                size: probe_bytes(),
                usage: BufferUsage::TRANSFER_SRC,
                memory: MemoryLocation::HostUpload,
            })
            .expect("a sentinel buffer");
        let sentinel_bytes: Vec<u8> = core::iter::repeat_n(PROBE_SENTINEL, PROBE_ELEMENTS as usize)
            .flat_map(u32::to_le_bytes)
            .collect();
        device
            .write_buffer(sentinel, 0, &sentinel_bytes)
            .expect("write");

        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("compute probe upload"),
            queue: headless.queue,
        });
        encoder.copy_buffer_to_buffer(&crcbl::hal::BufferCopy {
            src: upload,
            src_offset: 0,
            dst: params_buffer,
            dst_offset: 0,
            size: params.len() as u64,
        });
        encoder.copy_buffer_to_buffer(&crcbl::hal::BufferCopy {
            src: upload,
            src_offset: params.len() as u64,
            dst: source,
            dst_offset: 0,
            size: probe_bytes(),
        });
        encoder.pipeline_barrier(&Barriers {
            buffers: &[
                crcbl::hal::BufferBarrier {
                    buffer: params_buffer,
                    from: ResourceState::TransferDst,
                    to: ResourceState::ShaderRead,
                    queue_transfer: None,
                },
                crcbl::hal::BufferBarrier {
                    buffer: source,
                    from: ResourceState::TransferDst,
                    to: ResourceState::ShaderRead,
                    queue_transfer: None,
                },
            ],
            ..Barriers::default()
        });
        let commands = encoder.finish().expect("recording succeeded");
        device
            .submit(headless.queue, &SubmitInfo::new(&[commands]))
            .expect("submit");
        device.wait_idle().expect("idle");
        device.destroy_command_buffer(commands);
        device.destroy_buffer(upload);

        let layout_entries = [
            crcbl::hal::BindGroupLayoutEntry {
                binding: 0,
                visibility: crcbl::hal::ShaderStages::COMPUTE,
                kind: crcbl::hal::BindingKind::UniformBuffer { dynamic: false },
                count: 1,
                flags: crcbl::hal::BindingFlags::empty(),
            },
            crcbl::hal::BindGroupLayoutEntry {
                binding: 1,
                visibility: crcbl::hal::ShaderStages::COMPUTE,
                kind: crcbl::hal::BindingKind::StorageBuffer {
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: crcbl::hal::BindingFlags::empty(),
            },
            crcbl::hal::BindGroupLayoutEntry {
                binding: 2,
                visibility: crcbl::hal::ShaderStages::COMPUTE,
                kind: crcbl::hal::BindingKind::StorageBuffer {
                    read_only: false,
                    dynamic: false,
                },
                count: 1,
                flags: crcbl::hal::BindingFlags::empty(),
            },
        ];
        let bind_group_layout = device
            .create_bind_group_layout(&crcbl::hal::BindGroupLayoutDesc {
                label: Some("compute probe"),
                entries: &layout_entries,
            })
            .expect("the probe's layout");

        let group_entries = [
            crcbl::hal::BindGroupEntry {
                binding: 0,
                array_index: 0,
                resource: crcbl::hal::BindingResource::whole_buffer(params_buffer),
            },
            crcbl::hal::BindGroupEntry {
                binding: 1,
                array_index: 0,
                resource: crcbl::hal::BindingResource::whole_buffer(source),
            },
            crcbl::hal::BindGroupEntry {
                binding: 2,
                array_index: 0,
                resource: crcbl::hal::BindingResource::whole_buffer(destination),
            },
        ];
        let bind_group = device
            .create_bind_group(&crcbl::hal::BindGroupDesc {
                label: Some("compute probe"),
                layout: bind_group_layout,
                entries: &group_entries,
                variable_count: None,
            })
            .expect("a bind group");

        let set_layouts = [bind_group_layout];
        let pipeline_layout = device
            .create_pipeline_layout(&crcbl::hal::PipelineLayoutDesc {
                label: Some("compute probe"),
                bind_group_layouts: &set_layouts,
                push_constants: None,
            })
            .expect("a pipeline layout");

        // All four blobs, because all four backends run this file. The vk and
        // mtl copies this came from each passed only their own and `&[]` for the
        // rest, which is exactly the shape that let the suites diverge.
        let module = device
            .create_shader_module(&crcbl::hal::ShaderModuleDesc {
                label: Some("compute_probe.slang"),
                spirv: crcbl::shaders::COMPUTE_PROBE.spirv(),
                wgsl: crcbl::shaders::COMPUTE_PROBE.wgsl(),
                msl: crcbl::shaders::COMPUTE_PROBE.msl(),
                dxil: &crcbl::shaders::COMPUTE_PROBE.dxil_containers(),
            })
            .expect("the committed shader is accepted");
        // The manifest's name rather than a literal: it is read out of the
        // artifact's own entry-point table by the compile script, so a Slang
        // release that renamed it would fail here rather than in a driver.
        let entry_point = crcbl::shaders::COMPUTE_PROBE
            .entry_point(crcbl::shaders::Stage::Compute)
            .expect("the probe has exactly one compute entry point");
        let pipeline = device
            .create_compute_pipeline(&crcbl::hal::ComputePipelineDesc {
                label: Some("compute probe"),
                layout: pipeline_layout,
                compute: crcbl::hal::ShaderEntry {
                    module,
                    entry_point,
                },
                // The shader's own number, not a literal: `crcbl-shaders` checks
                // this constant against the `[numthreads(…)]` in
                // `compute_probe.slang`, and the backend checks it again against
                // what it is compiling.
                workgroup_size: [crcbl::shaders::compute_probe::WORKGROUP_SIZE, 1, 1],
            })
            .expect("a compute pipeline");
        device.destroy_shader_module(module);

        Self {
            params: params_buffer,
            source,
            destination,
            staging,
            sentinel,
            bind_group_layout,
            bind_group,
            pipeline_layout,
            pipeline,
        }
    }

    /// Fills the destination with the sentinel, runs `record` inside a compute
    /// pass, and reads the destination back.
    ///
    /// `record` is the *only* thing that varies between the dispatch and the
    /// indirect-dispatch tests, so both go through the same barriers and the
    /// same readback and a difference in the result is a difference in the
    /// dispatch.
    fn run(
        &self,
        headless: &Headless,
        record: impl FnOnce(&mut dyn crcbl::hal::CommandEncoder),
    ) -> Vec<u32> {
        let device = headless.device.as_ref();
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("compute probe dispatch"),
            queue: headless.queue,
        });
        let buffer_barrier = |buffer, from, to| crcbl::hal::BufferBarrier {
            buffer,
            from,
            to,
            queue_transfer: None,
        };
        // `TransferSrc` as the source state is vacuous on the first run and is
        // the real prior use on every later one — a buffer barrier carries no
        // layout, so naming a wider source scope than happened costs a stage
        // mask and cannot be wrong.
        encoder.pipeline_barrier(&Barriers {
            buffers: &[buffer_barrier(
                self.destination,
                ResourceState::TransferSrc,
                ResourceState::TransferDst,
            )],
            ..Barriers::default()
        });
        // A copy from a host buffer already full of the sentinel rather than
        // `fill_buffer`, which is the one adaptation this test needed to become
        // backend-agnostic: `crcbl-vk`'s copy filled with
        // [`PROBE_SENTINEL`] directly, and wgpu refuses a non-zero fill outright
        // — it only offers `clear_buffer`, and the seam reports that as
        // `HalError::Unsupported`. A zero fill would defeat the whole point,
        // because zero is exactly the value an unwritten destination is
        // indistinguishable from.
        encoder.copy_buffer_to_buffer(&crcbl::hal::BufferCopy {
            src: self.sentinel,
            src_offset: 0,
            dst: self.destination,
            dst_offset: 0,
            size: probe_bytes(),
        });
        // `ShaderReadWrite`, not `ShaderWrite`, even though `compute_probe.slang`
        // only ever assigns to `destination`. A barrier names the access the
        // *descriptor* permits rather than the one the source performs, and a
        // read-write storage binding is read-write unless the shader marks it
        // non-readable — which Slang does not emit for an `RWStructuredBuffer`.
        // Naming the write alone leaves the fill's transfer write unsynchronised
        // against a read the driver is entitled to make.
        encoder.pipeline_barrier(&Barriers {
            buffers: &[buffer_barrier(
                self.destination,
                ResourceState::TransferDst,
                ResourceState::ShaderReadWrite,
            )],
            ..Barriers::default()
        });

        encoder.begin_compute_pass(&crcbl::hal::ComputePassDesc {
            label: Some("compute probe"),
            timestamp_writes: None,
        });
        encoder.bind_compute_pipeline(self.pipeline);
        // Inside the pass, because the open scope is the only signal the seam
        // gives the backend about which bind point a group is for.
        encoder.bind_group(0, self.bind_group, &[], self.pipeline_layout);
        record(encoder.as_mut());
        encoder.end_compute_pass();

        encoder.pipeline_barrier(&Barriers {
            buffers: &[buffer_barrier(
                self.destination,
                ResourceState::ShaderReadWrite,
                ResourceState::TransferSrc,
            )],
            ..Barriers::default()
        });
        encoder.copy_buffer_to_buffer(&crcbl::hal::BufferCopy {
            src: self.destination,
            src_offset: 0,
            dst: self.staging,
            dst_offset: 0,
            size: probe_bytes(),
        });
        let commands = encoder.finish().expect("recording succeeded");
        device
            .submit(headless.queue, &SubmitInfo::new(&[commands]))
            .expect("submit");
        device.wait_idle().expect("idle");
        device.destroy_command_buffer(commands);

        let mut bytes = poisoned(probe_bytes() as usize);
        headless.readback(self.staging, probe_bytes(), &mut bytes);
        bytes
            .chunks_exact(4)
            .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
            .collect()
    }

    fn destroy(self, device: &dyn Device) {
        device.destroy_compute_pipeline(self.pipeline);
        device.destroy_pipeline_layout(self.pipeline_layout);
        device.destroy_bind_group(self.bind_group);
        device.destroy_bind_group_layout(self.bind_group_layout);
        device.destroy_buffer(self.sentinel);
        device.destroy_buffer(self.staging);
        device.destroy_buffer(self.destination);
        device.destroy_buffer(self.source);
        device.destroy_buffer(self.params);
    }
}

/// Compares a probe result against what the CPU says it should be, and says
/// which element disagreed first.
///
/// The element count is asserted before the values: a readback that came back
/// short would otherwise satisfy a `zip` over nothing at all.
fn assert_probe(actual: &[u32], expected: &[u32], what: &str) {
    assert_eq!(
        actual.len(),
        PROBE_ELEMENTS as usize,
        "{what}: the readback is not the whole destination buffer"
    );
    assert_eq!(expected.len(), actual.len(), "{what}: expectation length");
    if let Some((index, (got, want))) = actual
        .iter()
        .zip(expected)
        .enumerate()
        .find(|(_, (got, want))| got != want)
    {
        panic!(
            "{what}: element {index} is {got} ({got:#x}), expected {want} ({want:#x}). {} of \
             {} elements were expected to be written.",
            expected.iter().filter(|v| **v != PROBE_SENTINEL).count(),
            expected.len()
        );
    }
}

/// A dispatch that really ran, and really wrote the values it was asked for.
///
/// Migrated from `crcbl-vk`'s `vk_e2e/compute.rs`. The distinction it exists
/// for: `dispatch` returns nothing, so a backend that recorded no dispatch at
/// all would submit cleanly, present cleanly and leave a buffer full of
/// [`PROBE_SENTINEL`]. Only reading the destination back tells the two apart.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-hal-seam-e2e.sh"]
fn a_compute_dispatch_writes_the_values_it_was_asked_for() {
    let headless = Headless::open();
    let device = headless.device.as_ref();
    // Not a skip. `Features::COMPUTE` exists for a wgpu fallback that has no
    // compute at all; a device that opened a graphics queue and reports no
    // compute on any other backend is a capability-reporting bug, not a machine
    // to tiptoe around.
    assert!(
        device.caps().features.contains(Features::COMPUTE),
        "this device reports no compute; caps say {:?}",
        device.caps().features
    );

    let probe = ComputeProbe::new(&headless);
    let values = probe.run(&headless, |encoder| {
        encoder.dispatch(PROBE_GROUPS, 1, 1);
    });

    assert_probe(&values, &probe_expected(PROBE_ELEMENTS), "a full dispatch");
    assert!(
        !values.contains(&PROBE_SENTINEL),
        "a full dispatch must leave no element unwritten"
    );

    // The second arm the wgpu and dx12 copies carried and the Metal one did not.
    let empty = probe.run(&headless, |_| {});
    assert!(
        empty.iter().all(|value| *value == PROBE_SENTINEL),
        "a compute pass with no dispatch in it must write nothing, or the assertion above \
         was about the sentinel copy rather than about the dispatch; got {:?}…",
        &empty[..4]
    );

    probe.destroy(device);
    headless.finish();
}

/// `dispatch_indirect` reads its workgroup count out of GPU memory, at the
/// offset it was given.
///
/// Migrated from `crcbl-vk`'s `vk_e2e/compute.rs`. The argument buffer carries a
/// **decoy** at offset zero that would dispatch every workgroup, so three
/// different failures are distinguishable here rather than confusable: a backend
/// that ignored the offset dispatches eight groups and overwrites the tail; a
/// backend that ignored the argument buffer entirely writes nothing; a correct
/// one writes exactly the front of the buffer and leaves the sentinel behind it.
///
/// # The argument encoding
///
/// `crcbl-hal` does not spell the dispatch-argument layout —
/// [`CommandEncoder::dispatch_indirect`](crcbl::hal::CommandEncoder::dispatch_indirect)
/// names a buffer and an offset and nothing else — so this test writes the one
/// every backend's native API already fixes: three little-endian `u32`s, `x`,
/// `y`, `z`. That is `VkDispatchIndirectCommand`, `D3D12_DISPATCH_ARGUMENTS`,
/// `MTLDispatchThreadgroupsIndirectArguments` and WebGPU's
/// `dispatchWorkgroupsIndirect` block, all four identical. A backend whose
/// encoding differed would fail this test, which is the right place to find out.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-hal-seam-e2e.sh"]
fn an_indirect_dispatch_reads_its_workgroup_count_from_the_buffer() {
    let headless = Headless::open();
    let device = headless.device.as_ref();
    let probe = ComputeProbe::new(&headless);

    /// Workgroups the real arguments ask for. Fewer than [`PROBE_GROUPS`], so
    /// the difference is visible in the readback.
    const DISPATCHED_GROUPS: u32 = 2;
    /// Where the real arguments live. Non-zero, and the decoy sits at zero.
    const ARGS_OFFSET: u64 = 16;

    let mut args_bytes = vec![0u8; ARGS_OFFSET as usize + 12];
    for (slot, value) in [PROBE_GROUPS, 1, 1].iter().enumerate() {
        args_bytes[slot * 4..slot * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    for (slot, value) in [DISPATCHED_GROUPS, 1, 1].iter().enumerate() {
        let at = ARGS_OFFSET as usize + slot * 4;
        args_bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    let upload = device
        .create_buffer(&BufferDesc {
            label: Some("dispatch args upload"),
            size: args_bytes.len() as u64,
            usage: BufferUsage::TRANSFER_SRC,
            memory: MemoryLocation::HostUpload,
        })
        .expect("a staging buffer");
    device.write_buffer(upload, 0, &args_bytes).expect("write");
    let args = device
        .create_buffer(&BufferDesc {
            label: Some("dispatch args"),
            size: args_bytes.len() as u64,
            usage: BufferUsage::INDIRECT | BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("an indirect buffer");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("dispatch args upload"),
        queue: headless.queue,
    });
    encoder.copy_buffer_to_buffer(&crcbl::hal::BufferCopy {
        src: upload,
        src_offset: 0,
        dst: args,
        dst_offset: 0,
        size: args_bytes.len() as u64,
    });
    encoder.pipeline_barrier(&Barriers {
        buffers: &[crcbl::hal::BufferBarrier {
            buffer: args,
            from: ResourceState::TransferDst,
            to: ResourceState::IndirectArgument,
            queue_transfer: None,
        }],
        ..Barriers::default()
    });
    let commands = encoder.finish().expect("recording succeeded");
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");
    device.wait_idle().expect("idle");
    device.destroy_command_buffer(commands);
    device.destroy_buffer(upload);

    let values = probe.run(&headless, |encoder| {
        encoder.dispatch_indirect(args, ARGS_OFFSET);
    });

    let dispatched = DISPATCHED_GROUPS * crcbl::shaders::compute_probe::WORKGROUP_SIZE;
    assert!(
        dispatched > 0 && dispatched < PROBE_ELEMENTS,
        "the indirect dispatch must cover part of the buffer, not none and not all"
    );
    assert_probe(&values, &probe_expected(dispatched), "an indirect dispatch");
    // Said again in its own words, because the two halves fail for different
    // reasons: the front proves work happened, the tail proves the *count* came
    // from the buffer at the offset that was named.
    assert!(
        values[..dispatched as usize]
            .iter()
            .all(|value| *value != PROBE_SENTINEL),
        "the dispatched workgroups wrote nothing"
    );
    assert!(
        values[dispatched as usize..]
            .iter()
            .all(|value| *value == PROBE_SENTINEL),
        "the workgroups past the indirect count ran anyway — the argument buffer or its \
         offset was not honoured"
    );

    device.destroy_buffer(args);
    probe.destroy(device);
    headless.finish();
}

// --- handles, ranges and refusals ------------------------------------------

/// A destroyed handle does not alias the buffer that takes its slot.
///
/// Migrated from three near-verbatim copies — `crcbl-wgpu`'s `wgpu_e2e.rs`,
/// `crcbl-mtl`'s `device.rs` and `crcbl-dx12`'s `device.rs`. The wgpu copy is
/// the one kept, because it is the only one that pinned the `kind` and stated
/// the stale-versus-foreign distinction in its message; the other two accepted
/// any [`HalError::InvalidHandle`], which a foreign-object bug would also
/// satisfy.
///
/// Both halves are the test: the slot really is recycled (so this is exercising
/// recycling at all), and the generation really moved (so the old handle is
/// distinguishable from the new one).
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-hal-seam-e2e.sh"]
fn a_destroyed_handle_does_not_alias_the_buffer_that_replaces_it() {
    let instance = instance();
    let adapter = select_adapter(instance.as_ref());
    let device = open_headless_device(instance.as_ref(), adapter.id);

    let desc = BufferDesc {
        label: Some("recycled"),
        size: 256,
        usage: BufferUsage::TRANSFER_SRC,
        memory: MemoryLocation::HostUpload,
    };
    let first = device.create_buffer(&desc).expect("a buffer");
    device.destroy_buffer(first);
    let second = device.create_buffer(&desc).expect("the slot is reused");

    assert_eq!(
        first.index(),
        second.index(),
        "the new buffer did not take the destroyed one's slot, so this test is not \
         exercising recycling at all"
    );
    assert_ne!(
        first, second,
        "the slot was reused and the generation never moved, so the destroyed handle now \
         names the buffer that replaced it"
    );

    device
        .write_buffer(second, 0, &[1, 2, 3, 4])
        .expect("the live handle resolves");
    let error = device
        .write_buffer(first, 0, &[1, 2, 3, 4])
        .expect_err("the destroyed handle must not resolve to its replacement");
    assert!(
        matches!(error, HalError::InvalidHandle { kind: "buffer", .. }),
        "a stale handle of this device's own is stale, not foreign: {error}"
    );

    device.destroy_buffer(second);
    assert_no_out_of_band_error(device.as_ref());
    drop(device);
}

/// `write_buffer` refuses what it cannot map, and refuses a range that does not
/// fit.
///
/// Migrated from `crcbl-mtl`'s `device.rs` and `crcbl-dx12`'s `device.rs`, which
/// carried the same test twice. What did **not** come with it is each copy's
/// assertion about the refusal's *wording* — mtl required the message to contain
/// `"blit"` and dx12 `"copy"`, which is the same claim spelled in two backends'
/// vocabularies and cannot be asserted once. The variant is the seam's; the
/// prose is the backend's.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-hal-seam-e2e.sh"]
fn write_buffer_writes_host_visible_memory_and_refuses_what_it_cannot_map() {
    let instance = instance();
    let adapter = select_adapter(instance.as_ref());
    let device = open_headless_device(instance.as_ref(), adapter.id);

    let upload = device
        .create_buffer(&BufferDesc {
            label: Some("upload"),
            size: 16,
            usage: BufferUsage::TRANSFER_SRC,
            memory: MemoryLocation::HostUpload,
        })
        .expect("a host-upload buffer");
    device
        .write_buffer(upload, 0, &[0xAA; 16])
        .expect("a host-upload buffer is what this call is for");
    device
        .write_buffer(upload, 4, &[1, 2, 3, 4])
        .expect("a window inside the buffer is still inside it");

    let device_local = device
        .create_buffer(&BufferDesc {
            label: Some("device local"),
            size: 16,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("a device-local buffer");
    let error = device
        .write_buffer(device_local, 0, &[1, 2, 3, 4])
        .expect_err(
            "device-local memory is not mappable, and guessing a staging copy is not \
                     this call's job",
        );
    assert!(
        matches!(error, HalError::InvalidDescriptor(_)),
        "an unmappable destination is a caller bug named in the descriptor, not a driver \
         failure: {error}"
    );

    // Four bytes at offset 13 run three past the end of a sixteen-byte buffer.
    let error = device
        .write_buffer(upload, 13, &[1, 2, 3, 4])
        .expect_err("a write that runs past the end must be refused, not truncated");
    assert!(
        matches!(error, HalError::InvalidDescriptor(_)),
        "an out-of-range write is a caller bug named in the descriptor: {error}"
    );

    device.destroy_buffer(device_local);
    device.destroy_buffer(upload);
    assert_no_out_of_band_error(device.as_ref());
    drop(device);
}

/// `request_readback` refuses the buffers and ranges it cannot serve, rather
/// than panicking or handing back a request that never completes.
///
/// Migrated from `crcbl-wgpu`'s `wgpu_e2e.rs` and `crcbl-dx12`'s `device.rs`,
/// which each covered part of this and neither all of it; `crcbl-mtl` had no
/// equivalent at all, so on Metal every one of these was unasserted.
///
/// The three cases fail for three different reasons and are therefore three
/// assertions rather than a loop with one message: a destination that is not
/// host-readable at all, a range past the end, and a range whose end overflows.
///
/// **An empty range is deliberately not here.** `crcbl-wgpu`'s copy refused
/// `size: 0` and `crcbl-vk` serves it, and the seam is on Vulkan's side:
/// [`Device::request_readback`] promises
/// [`HalError::InvalidDescriptor`](crcbl::hal::HalError::InvalidDescriptor)
/// for a range that is *out of bounds*, and an empty range is in bounds. wgpu's
/// extra strictness comes from `mapAsync`, so it stays in wgpu's own suite.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-hal-seam-e2e.sh"]
fn a_readback_of_the_wrong_buffer_or_range_is_refused_instead_of_panicking() {
    let instance = instance();
    let adapter = select_adapter(instance.as_ref());
    let device = open_headless_device(instance.as_ref(), adapter.id);

    let readable = device
        .create_buffer(&BufferDesc {
            label: Some("readback"),
            size: 64,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a host-readback buffer");
    let device_local = device
        .create_buffer(&BufferDesc {
            label: Some("device local"),
            size: 256,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("a device-local buffer");

    let refused = |what: &str, buffer, offset: u64, size: u64| {
        let error = device
            .request_readback(&ReadbackDesc {
                label: Some("refused"),
                buffer,
                offset,
                size,
                after: None,
            })
            .err()
            .unwrap_or_else(|| panic!("{what} must be refused, not served"));
        assert!(
            matches!(error, HalError::InvalidDescriptor(_)),
            "{what}: a readback the seam cannot serve is a caller bug named in the \
             descriptor: {error}"
        );
    };
    refused(
        "a device-local source, which the host cannot read",
        device_local,
        0,
        64,
    );
    refused("a range past the end of the buffer", readable, 0, 128);
    refused("a range whose end overflows", readable, u64::MAX, 8);

    device.destroy_buffer(device_local);
    device.destroy_buffer(readable);
    assert_no_out_of_band_error(device.as_ref());
    drop(device);
}

/// Destroying a readback whose bytes never landed is legal and reports nothing.
///
/// **The shape of a bug that reached a user.** On WebGPU `destroy_readback` is
/// `unmap()`, and unmapping a buffer whose `mapAsync` is still resolving rejects
/// that promise with an `AbortError` — which, filed as a device error, reads as
/// `readback N could not be mapped: AbortError` and names a call that did
/// exactly what it was told. The other four backends satisfy this by accident:
/// they drop a tracking entry and there is no promise to reject. So this is the
/// obligation WebGPU had to be told about, and the one an agnostic test has to
/// hold every backend to rather than trusting four of them to keep being
/// incidentally right.
///
/// The readback is destroyed **without ever being polled to
/// [`ReadbackState::Ready`]**, which is the whole point: a poll that reached
/// `Ready` has nothing outstanding to cancel, so a test that polled first would
/// pass on a backend that surfaced the cancellation loudly.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-hal-seam-e2e.sh"]
fn destroying_a_readback_before_its_bytes_land_reports_nothing() {
    let instance = instance();
    let adapter = select_adapter(instance.as_ref());
    let device = open_headless_device(instance.as_ref(), adapter.id);

    let readable = device
        .create_buffer(&BufferDesc {
            label: Some("cancelled readback"),
            size: 64,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a host-readback buffer");

    let readback = device
        .request_readback(&ReadbackDesc {
            label: Some("destroyed while still mapping"),
            buffer: readable,
            offset: 0,
            size: 64,
            after: None,
        })
        .expect("a readback request");
    device.destroy_readback(readback);

    // And a second one afterwards still works, so the cancel did not leave the
    // backend's readback bookkeeping wedged — the failure a "no error was
    // reported" assertion alone would sit happily beside.
    let mut out = [0u8; 64];
    let again = device
        .request_readback(&ReadbackDesc {
            label: Some("after a cancelled one"),
            buffer: readable,
            offset: 0,
            size: 64,
            after: None,
        })
        .expect("a readback request after a cancelled one");
    let deadline = Instant::now() + READBACK_DEADLINE;
    loop {
        match device
            .poll_readback(again, &mut out)
            .expect("the second readback did not fail")
        {
            ReadbackState::Ready => break,
            ReadbackState::Pending => assert!(
                Instant::now() < deadline,
                "a readback issued after a cancelled one never completed, so the cancel left \
                 the backend's bookkeeping wedged"
            ),
        }
        std::thread::yield_now();
    }
    device.destroy_readback(again);

    device.destroy_buffer(readable);
    assert_no_out_of_band_error(device.as_ref());
    drop(device);
}

/// Presenting an image nothing acquired is refused.
///
/// Migrated from `crcbl-wgpu`'s `wgpu_e2e.rs` and `crcbl-mtl`'s `swapchain.rs`.
/// Neither copy's message assertion came with it — wgpu required the text to
/// contain `"without a matching acquire"` and mtl `"acquire_next_frame"`, which
/// is one claim asserted through two spellings. `crcbl-vk` and `crcbl-dx12`
/// implement the refusal and had no test for it at all, so this is the first
/// time either is held to it.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-hal-seam-e2e.sh"]
fn presenting_a_swapchain_without_an_acquire_is_refused() {
    let headless = Headless::open();

    let error = headless
        .device
        .present(
            headless.queue,
            &PresentInfo {
                swapchain: headless.swapchain,
                waits: &[],
                present_id: None,
            },
        )
        .expect_err("there is no acquired image to present");
    assert!(
        matches!(error, SurfaceError::Hal(HalError::InvalidDescriptor(_))),
        "presenting nothing is a caller bug named in the descriptor, not an out-of-date \
         swapchain: {error}"
    );

    headless.finish();
}

/// A pipeline layout asking for **every reported ceiling at once** is served or
/// refused by name, and never quietly cut down.
///
/// # The claim this makes, which is not the one `Limits` used to imply
///
/// [`crcbl::hal::Limits`] bounds each quantity on its own, and three backends
/// spend several of them out of one budget the seam has no field for: a D3D12
/// root signature costs at most `D3D12_MAX_ROOT_COST` DWORDs for the whole
/// thing, with every set's tables *and* the push-constant range coming out of
/// it; Metal lowers the push-constant block to a buffer argument competing with
/// the sets for one per-stage argument table; WebGPU caps binding counts per
/// shader stage, which `Limits` has no field for at all. So
/// `max_bind_groups` sets **and** `max_push_constant_size` bytes — the one
/// request that takes every field as independently reachable — is legal to ask
/// for and legal to refuse, and the seam's promise is about the *shape* of the
/// answer rather than which answer arrives.
///
/// **Both arms are the assertion, and neither alone would be one.** Success
/// alone is what Vulkan gives and says nothing about D3D12, where this layout
/// genuinely does not fit; a refusal alone is what no backend here gives. What
/// is forbidden is every third answer: another [`HalError`] variant, a panic out
/// of the backend, or an `Ok` covering a layout the API underneath rejected —
/// which is what [`Headless::finish`]'s
/// [`take_error`](crcbl::hal::Device::take_error) check catches, since on wgpu
/// and WebGPU that is where such an acceptance surfaces.
///
/// # Why the wide fixture, and why the numbers are printed
///
/// `max_push_constant_size` is `0` on a device without
/// [`Features::PUSH_CONSTANTS`], and the narrow ask does not request them — so
/// [`Headless::open`] would make the push-constant half of this vacuous on
/// hardware that has them, while still passing. The ceilings actually reached
/// are printed for the same reason: a run says which numbers it asked for
/// instead of leaving that to be assumed from a green tick.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-hal-seam-e2e.sh"]
fn a_pipeline_layout_at_every_reported_ceiling_is_served_or_refused_by_name() {
    let headless = Headless::open_with_every_optional_feature();
    let device = headless.device.as_ref();
    let limits = device.caps().limits;

    // One non-dynamic uniform buffer per set: the cheapest set that still costs
    // something, which on D3D12 is one descriptor table and so one DWORD of the
    // signature. Sets holding nothing would cost nothing there and the joint
    // budget would never be reached.
    let entries = [crcbl::hal::BindGroupLayoutEntry {
        binding: 0,
        visibility: crcbl::hal::ShaderStages::COMPUTE,
        kind: crcbl::hal::BindingKind::UniformBuffer { dynamic: false },
        count: 1,
        flags: crcbl::hal::BindingFlags::empty(),
    }];
    let set_layouts: Vec<crcbl::hal::BindGroupLayoutHandle> = (0..limits.max_bind_groups)
        .map(|set| {
            device
                .create_bind_group_layout(&crcbl::hal::BindGroupLayoutDesc {
                    label: Some("ceiling set"),
                    entries: &entries,
                })
                .unwrap_or_else(|error| {
                    panic!(
                        "set {set} of the {} this device reports as max_bind_groups could not be \
                         created at all — {error}. One uniform buffer is the smallest layout \
                         there is, so this is not the joint-budget refusal under test.",
                        limits.max_bind_groups
                    )
                })
        })
        .collect();

    // `0` is what a device without push constants reports, and an empty range is
    // not a range — `None` is what asking for the whole of that ceiling means.
    let push_constants =
        (limits.max_push_constant_size > 0).then_some(crcbl::hal::PushConstantRange {
            stages: crcbl::hal::ShaderStages::COMPUTE,
            offset: 0,
            size: limits.max_push_constant_size,
        });
    eprintln!(
        "crcbl hal seam e2e: a pipeline layout at max_bind_groups={} sets and \
         max_push_constant_size={} bytes",
        limits.max_bind_groups, limits.max_push_constant_size,
    );

    match device.create_pipeline_layout(&crcbl::hal::PipelineLayoutDesc {
        label: Some("every ceiling at once"),
        bind_group_layouts: &set_layouts,
        push_constants,
    }) {
        Ok(layout) => {
            eprintln!(
                "crcbl hal seam e2e:   served, so this backend's fields are jointly reachable"
            );
            device.destroy_pipeline_layout(layout);
        }
        Err(HalError::InvalidDescriptor(reason)) => {
            eprintln!("crcbl hal seam e2e:   refused: {reason}");
        }
        Err(error) => panic!(
            "a layout inside every reported limit was refused with {error}. A budget this seam \
             has no field for is the caller's descriptor being too large for the backend, which \
             is InvalidDescriptor with the arithmetic in it — Unsupported would say the device \
             lacks a capability, and InvalidHandle would say these layouts are not live."
        ),
    }

    for layout in set_layouts {
        device.destroy_bind_group_layout(layout);
    }
    headless.finish();
}

// --- the timeline ----------------------------------------------------------

/// How long the timeline test waits before calling the signal lost.
///
/// Nanoseconds, because that is [`Device::wait_semaphores`]'s unit.
const SEMAPHORE_DEADLINE_NS: u64 = 20_000_000_000;

/// A timeline semaphore signalled by a submission, seen by the CPU, and refusing
/// to go backwards.
///
/// Migrated from `crcbl-wgpu`'s `wgpu_e2e.rs` and `crcbl-mtl`'s `device.rs`,
/// which carried the same sequence with the same initial and signalled values.
/// Both arms are here rather than a skip, because a backend that hands out a
/// timeline it cannot drive and a backend that says so are the two answers this
/// has to tell apart — `crcbl-dx12` is the one that says so today, and until
/// this existed that refusal was asserted only inside D3D12's own crate.
///
/// # Two halves the copies disagreed on, and so are not here
///
/// **A wait nothing will satisfy.** wgpu answers `Err(Unsupported)` and Metal
/// answers `Ok(false)`. The seam is explicit that `Ok(false)` is the contract —
/// [`Device::wait_semaphores`] calls a timeout "a normal outcome for a
/// frame-pacing poll, not an error" — so wgpu is the one that is wrong, and
/// asserting it here would go red on wgpu for a wgpu bug rather than a seam one.
///
/// **Signalling a value the timeline already holds.** wgpu and Metal both refuse
/// it with [`HalError::InvalidDescriptor`]; `crcbl-vk` accepts the submission and
/// passes it to the driver, where the validation layer reports
/// `VUID-VkSubmitInfo2-semaphore-03882`. A monotonicity violation deadlocks
/// everything waiting past that value, so this is worth holding every backend
/// to — but it needs `crcbl-vk` to check it first.
///
/// Both stay in the backends' own suites until those are settled. They are
/// recorded here rather than dropped silently, because a behaviour that quietly
/// left a shared suite is a behaviour nobody will notice is missing.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-hal-seam-e2e.sh"]
fn a_timeline_semaphore_signals_from_a_submission_and_the_cpu_sees_it() {
    let headless = Headless::open();
    let has_timeline = headless
        .device
        .caps()
        .features
        .contains(Features::TIMELINE_SEMAPHORE);

    for claim in [TimelineClaim::Counter, TimelineClaim::CpuWait] {
        match exercise_timeline(&headless, claim) {
            Exercise::Worked => assert!(
                has_timeline,
                "{claim:?}: this device reports no TIMELINE_SEMAPHORE and the timeline ran \
                 anyway, so either the feature report or the refusal is wrong"
            ),
            Exercise::Refused(error) => {
                assert!(
                    !has_timeline,
                    "{claim:?}: this device reports TIMELINE_SEMAPHORE and then refused it — \
                     {error}"
                );
                assert!(
                    matches!(error, HalError::Unsupported { .. }),
                    "{claim:?}: a backend without timelines refuses by name, so a caller can \
                     branch on it rather than discovering a semaphore that never signals: {error}"
                );
            }
            outcome => panic!(
                "{claim:?}: the timeline was driven as {outcome:?} on a device that reports \
                 TIMELINE_SEMAPHORE = {has_timeline}"
            ),
        }
    }

    headless.finish();
}

/// Which half of the timeline [`exercise_timeline`] reports on.
///
/// The two are separate [`crcbl::hal::Capability`] variants because a backend
/// can hand out a counter a submission signals while having no host-side wait to
/// offer, so one exercise cannot answer for both — the *sequence* is shared, the
/// verdict is not.
#[derive(Clone, Copy, Debug)]
enum TimelineClaim {
    /// [`Capability::TimelineSemaphore`]: the counter a submission advances and
    /// [`Device::semaphore_value`] reports.
    Counter,
    /// [`Capability::CpuTimelineWait`]: [`Device::wait_semaphores`] blocking on
    /// the CPU until that counter arrives.
    CpuWait,
}

/// Drives one `claim` about a timeline semaphore and reports what happened.
///
/// Owns the sequence
/// [`a_timeline_semaphore_signals_from_a_submission_and_the_cpu_sees_it`] used
/// to spell inline, so the capability table and that test assert the same thing
/// rather than two copies of it: create with a non-zero initial value, signal a
/// higher one from a submission, and observe it.
///
/// # What each outcome means
///
/// * [`Exercise::Refused`] — `create_semaphore` would not hand out a timeline,
///   which is what `crcbl-dx12` does and what any device without
///   [`Features::TIMELINE_SEMAPHORE`] must do. Both claims refuse there for the
///   same reason: there is no counter to read and none to wait on.
/// * [`Exercise::SilentlyIgnored`] — a handle came back and the value never
///   arrived. [`Capability::TimelineSemaphore`]'s own documentation names this
///   as the failure it exists to catch ("a backend that hands out a handle whose
///   value never moves has not got this, whatever its return codes say"), and
///   the wait's equivalent is `Ok(false)`: the seam calls a timeout a normal
///   outcome, so it is not an error, but a wait that timed out on a value
///   already submitted observed nothing.
/// * [`Exercise::Worked`] — the counter moved to what the submission signalled.
fn exercise_timeline(headless: &Headless, claim: TimelineClaim) -> Exercise {
    /// What the semaphore is created holding. Non-zero, so a backend that
    /// ignored `initial_value` and started at zero is visible immediately.
    const INITIAL: u64 = 5;
    /// What the submission signals. Above [`INITIAL`], because a timeline only
    /// moves forwards.
    const SIGNALLED: u64 = 9;

    let device = headless.device.as_ref();
    let timeline = match device.create_semaphore(&crcbl::hal::SemaphoreDesc {
        label: Some("crcbl hal seam e2e timeline"),
        kind: crcbl::hal::SemaphoreKind::Timeline {
            initial_value: INITIAL,
        },
    }) {
        Ok(timeline) => timeline,
        Err(error) => return Exercise::Refused(error),
    };
    assert_eq!(
        device
            .semaphore_value(timeline)
            .expect("a timeline has one"),
        INITIAL,
        "the semaphore did not start at the value it was created with"
    );

    // Signal-only, with no command buffers: the claim is about the *timeline*,
    // and work in the submission would only add a way for this to fail for a
    // reason that is not the one under test.
    device
        .submit(
            headless.queue,
            &SubmitInfo {
                command_buffers: &[],
                waits: &[],
                signals: &[crcbl::hal::SemaphoreSignal {
                    semaphore: timeline,
                    value: SIGNALLED,
                }],
            },
        )
        .expect("a signal-only submission");

    let outcome = match claim {
        TimelineClaim::CpuWait => match device.wait_semaphores(
            &[crcbl::hal::SemaphoreWait {
                semaphore: timeline,
                value: SIGNALLED,
            }],
            SEMAPHORE_DEADLINE_NS,
        ) {
            Ok(true) => Exercise::Worked,
            Ok(false) => Exercise::SilentlyIgnored,
            Err(error) => Exercise::Refused(error),
        },
        // `wait_idle` rather than the wait above, so the counter is read on its
        // own: a backend whose CPU wait is the thing that is missing must still
        // fail this claim only if the *value* did not move.
        TimelineClaim::Counter => {
            device.wait_idle().expect("idle");
            match device.semaphore_value(timeline) {
                Ok(SIGNALLED) => Exercise::Worked,
                Ok(INITIAL) => Exercise::SilentlyIgnored,
                Ok(value) => panic!(
                    "the submission signalled {SIGNALLED} and the timeline reads {value}, which \
                     is neither that nor the {INITIAL} it was created with — the counter moved \
                     and moved to the wrong place"
                ),
                Err(error) => Exercise::Refused(error),
            }
        }
    };

    device.destroy_semaphore(timeline);
    outcome
}

/// Drives [`Device::signal_semaphore`] — the **host** advancing a timeline — and
/// reports what happened.
///
/// Its own sequence rather than a third arm of [`exercise_timeline`], because it
/// submits nothing at all: the claim is that a value the host asks for arrives,
/// and a submission in the way would only add a reason for this to fail that is
/// not the one under test.
///
/// # What each outcome means
///
/// * [`Exercise::Refused`] — either `create_semaphore` would not hand out a
///   timeline, or the backend has no host signal for one. `crcbl-wgpu` is the
///   second case: its counter is a record of what each submission will have
///   completed, so there is no object to signal.
/// * [`Exercise::SilentlyIgnored`] — the call returned `Ok` and the counter did
///   not move, which is the shape this whole mechanism exists to separate from a
///   working call.
/// * [`Exercise::Worked`] — the counter reads what the host signalled. The
///   forwards-only rule is asserted rather than scored, because a backend that
///   moves the counter and accepts a value moving it backwards has the
///   capability and a bug: scoring it as unsupported would say the wrong thing.
fn exercise_cpu_timeline_signal(headless: &Headless) -> Exercise {
    /// What the semaphore is created holding, so a backend that ignored
    /// `initial_value` is visible before the signal is ever sent.
    const INITIAL: u64 = 5;
    /// What the host signals. Above [`INITIAL`], because a timeline only moves
    /// forwards.
    const SIGNALLED: u64 = 9;

    let device = headless.device.as_ref();
    let timeline = match device.create_semaphore(&crcbl::hal::SemaphoreDesc {
        label: Some("crcbl hal seam e2e host signal"),
        kind: crcbl::hal::SemaphoreKind::Timeline {
            initial_value: INITIAL,
        },
    }) {
        Ok(timeline) => timeline,
        Err(error) => return Exercise::Refused(error),
    };

    let outcome = match device.signal_semaphore(timeline, SIGNALLED) {
        Err(error) => Exercise::Refused(error),
        Ok(()) => match device.semaphore_value(timeline) {
            Ok(SIGNALLED) => {
                // The rule the seam states for this call, on a backend that has
                // just shown it performs the call at all. Vulkan refuses a
                // value at or below the counter itself; D3D12 and Metal both
                // set one backwards without a word, and then every waiter past
                // the higher value stops waking on a healthy queue.
                for backwards in [0, INITIAL, SIGNALLED] {
                    let refusal = device.signal_semaphore(timeline, backwards);
                    assert!(
                        matches!(refusal, Err(HalError::InvalidDescriptor(_))),
                        "signalling {backwards} onto a timeline already holding {SIGNALLED} must \
                         be refused as InvalidDescriptor — the value is a number the caller can \
                         correct, not a capability the backend lacks: {refusal:?}"
                    );
                }

                // The other half of the same call's contract, and the half
                // nothing on a real backend held to it: a binary semaphore has
                // no value, so signalling one is `Unsupported` — the backend
                // genuinely cannot do it — and not `InvalidDescriptor`, which
                // would send a caller looking for a bad number in a call whose
                // numbers are all fine. `crcbl-hal`'s null backend asserted this
                // and no real one did, so vk could refuse it through the wrong
                // variant with every gate green.
                // A backend that hands out no binary semaphore at all owes
                // nothing here; `Capability::BinarySemaphore` is where that is
                // declared, and this exercise is not it.
                if let Ok(binary) = device.create_semaphore(&crcbl::hal::SemaphoreDesc {
                    label: Some("crcbl hal seam e2e binary signal"),
                    kind: crcbl::hal::SemaphoreKind::Binary,
                }) {
                    let refusal = device.signal_semaphore(binary, 1);
                    // **The same refusal from the other side**, which had no
                    // agnostic assertion at all until this one. A host *wait* on
                    // a binary semaphore has nothing to compare against for the
                    // same reason a host signal has nothing to advance, and
                    // `crcbl-vk`, `crcbl-mtl` and `crcbl-dx12` all refuse it in
                    // the same words — while `crcbl-hal`'s null backend answered
                    // `Ok(true)`, so a caller reading the seam's behaviour off
                    // the null device was told a wait no real device accepts had
                    // been satisfied.
                    let waited = device.wait_semaphores(
                        &[crcbl::hal::SemaphoreWait {
                            semaphore: binary,
                            value: 0,
                        }],
                        0,
                    );
                    device.destroy_semaphore(binary);
                    assert!(
                        matches!(refusal, Err(HalError::Unsupported { .. })),
                        "signalling a binary semaphore must be refused as Unsupported — it \
                         carries no value for a host signal to advance, which is a thing the \
                         backend cannot do rather than a number the caller got wrong: {refusal:?}"
                    );
                    assert!(
                        matches!(waited, Err(HalError::Unsupported { .. })),
                        "and waiting on one must be refused the same way, rather than reported \
                         as a wait that was satisfied or as a timeout the caller would retry \
                         forever: {waited:?}"
                    );
                }

                Exercise::Worked
            }
            Ok(INITIAL) => Exercise::SilentlyIgnored,
            Ok(value) => panic!(
                "the host signalled {SIGNALLED} and the timeline reads {value}, which is neither \
                 that nor the {INITIAL} it was created with — the counter moved and moved to the \
                 wrong place"
            ),
            Err(error) => Exercise::Refused(error),
        },
    };

    device.destroy_semaphore(timeline);
    outcome
}

/// Drives a submitted wait on a value **nothing submitted will signal**, and
/// opens it from the host.
///
/// # Why this cannot deadlock the suite
///
/// It used to be unexercised because a wait the backend accepted and could not
/// satisfy stops the queue with nothing in any log, and every later call in the
/// run blocks behind it. [`Device::signal_semaphore`] is what changed that, and
/// the order below is what makes it safe rather than merely likely:
///
/// 1. The host signals the gate **before anything is submitted**, and the
///    counter is read back to prove it moved. A backend with no host signal is
///    [`Exercise::Refused`] here and one whose signal does nothing is
///    [`Exercise::SilentlyIgnored`] — both having submitted nothing.
/// 2. Only then is the wait submitted — for the *next* value, which nothing on
///    the queue will ever produce.
/// 3. The host signals that value, using the call step 1 already proved works.
/// 4. The completion is observed through [`Device::wait_semaphores`] with
///    [`SEMAPHORE_DEADLINE_NS`], never `wait_idle`, so the only unbounded call
///    left in the sequence is `submit` — which no backend here blocks in.
///
/// # What each outcome means
///
/// * [`Exercise::Refused`] — no timeline, no host signal, or the backend refuses
///   the wait itself. `crcbl-wgpu` refuses twice over: its timeline is
///   per-submission completion, so there is nothing to signal and nothing for a
///   queue-side wait to block on.
/// * [`Exercise::SilentlyIgnored`] — the wait was accepted, the gate was opened,
///   and the submission's own signal never arrived inside the deadline.
/// * [`Exercise::Worked`] — the submission ran, which it could only do on the
///   value the host produced.
fn exercise_timeline_wait_before_signal(headless: &Headless) -> Exercise {
    /// What the host signals to prove it can, before anything is submitted.
    const OPENED: u64 = 1;
    /// What the submitted wait asks for. Above [`OPENED`], so nothing that has
    /// happened yet satisfies it and no submission ever will.
    const GATE: u64 = 2;

    let device = headless.device.as_ref();
    let timeline = |label| {
        device.create_semaphore(&crcbl::hal::SemaphoreDesc {
            label: Some(label),
            kind: crcbl::hal::SemaphoreKind::Timeline { initial_value: 0 },
        })
    };
    let gate = match timeline("crcbl hal seam e2e gate") {
        Ok(gate) => gate,
        Err(error) => return Exercise::Refused(error),
    };
    let done = match timeline("crcbl hal seam e2e gated completion") {
        Ok(done) => done,
        Err(error) => {
            device.destroy_semaphore(gate);
            return Exercise::Refused(error);
        }
    };

    let outcome = (|| {
        // Step 1: nothing is submitted until the host signal is known to have
        // *moved the counter*, not merely to have returned `Ok`. A signal that
        // answers `Ok` and does nothing is the one failure that would leave the
        // wait below unsatisfiable with a submission already on the queue, and
        // this is where it is caught — before there is a queue to stop.
        if let Err(error) = device.signal_semaphore(gate, OPENED) {
            return Exercise::Refused(error);
        }
        match device.semaphore_value(gate) {
            Ok(OPENED) => {}
            Ok(_) => return Exercise::SilentlyIgnored,
            Err(error) => return Exercise::Refused(error),
        }
        // Step 2: a wait for a value no submission will ever signal, carrying a
        // signal of its own so the completion has something to observe.
        if let Err(error) = device.submit(
            headless.queue,
            &SubmitInfo {
                command_buffers: &[],
                waits: &[crcbl::hal::SemaphoreWait {
                    semaphore: gate,
                    value: GATE,
                }],
                signals: &[crcbl::hal::SemaphoreSignal {
                    semaphore: done,
                    value: 1,
                }],
            },
        ) {
            return Exercise::Refused(error);
        }
        // Step 3: the same call as step 1, so a failure here is a backend that
        // signals once and not twice rather than one that cannot signal.
        device
            .signal_semaphore(gate, GATE)
            .expect("the host signal worked before the submission and must work after it");
        // Step 4: bounded, so a wait this backend cannot satisfy ends in a
        // verdict rather than in a hung run.
        match device.wait_semaphores(
            &[crcbl::hal::SemaphoreWait {
                semaphore: done,
                value: 1,
            }],
            SEMAPHORE_DEADLINE_NS,
        ) {
            Ok(true) => Exercise::Worked,
            Ok(false) => Exercise::SilentlyIgnored,
            Err(error) => Exercise::Refused(error),
        }
    })();

    device.destroy_semaphore(done);
    device.destroy_semaphore(gate);
    outcome
}

// --- the capability seam ----------------------------------------------------
//
// `crcbl_hal::Capability` is the enum every backend answers for through an
// exhaustive `match`, so a seam behaviour added to one backend fails to compile
// on the four that have not said what they do about it. That property is the
// compiler's. What the compiler cannot check is whether an answer is *true* —
// a backend claiming a capability it refuses at the call site, or refusing one
// it never declared, compiles perfectly. These three tests are that half.

/// What driving one [`Capability`] against a real device produced.
///
/// Four outcomes rather than two, because "accepted and did nothing" is a real
/// failure mode this seam has had — a fill that returns cleanly and writes no
/// bytes is indistinguishable from a working one at the return value, and only
/// reading the memory back tells them apart. It is never a pass: the seam's own
/// rule is that a backend fails loudly rather than dropping a request, so a
/// silent no-op is wrong whether the capability was declared or not.
#[derive(Debug)]
enum Exercise {
    /// The behaviour ran and produced what the seam documents.
    Worked,
    /// The behaviour was refused, carrying the error the backend gave.
    Refused(HalError),
    /// The call was accepted and nothing observable happened.
    SilentlyIgnored,
    /// Not drivable from this suite, with the reason stated rather than skipped.
    Unexercised(&'static str),
}

/// The byte pattern a fill destination holds before the fill, so that a fill
/// which did nothing is distinguishable from one that worked.
///
/// Not `0`, and not any of the fill values below: a destination that still holds
/// this after a `fill_buffer` proves the call was dropped rather than performed.
const FILL_POISON: u32 = 0x5A5A_5A5A;

/// Drives [`CommandEncoder::fill_buffer`](crcbl::hal::CommandEncoder::fill_buffer)
/// and reports what actually reached memory.
///
/// The destination is primed with [`FILL_POISON`] through a copy first, so the
/// three outcomes are separable: zero means it worked, the poison means the
/// call was accepted and dropped, and an error out of `finish` means it was
/// refused.
///
/// It took a `value` and was driven three times — with `0`, with four equal
/// bytes for Metal's byte-wide fill, and with four different ones — until the
/// seam's fill became a clear. The two valued capabilities went with it.
fn exercise_clear(headless: &Headless) -> Exercise {
    const BYTES: u64 = 16;
    let device = headless.device.as_ref();

    let prime = device
        .create_buffer(&BufferDesc {
            label: Some("fill prime"),
            size: BYTES,
            usage: BufferUsage::TRANSFER_SRC,
            memory: MemoryLocation::HostUpload,
        })
        .expect("a host-upload buffer");
    let target = device
        .create_buffer(&BufferDesc {
            label: Some("fill target"),
            size: BYTES,
            usage: BufferUsage::TRANSFER_DST | BufferUsage::TRANSFER_SRC,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("a device-local buffer");
    let staging = device
        .create_buffer(&BufferDesc {
            label: Some("fill staging"),
            size: BYTES,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a host-readback buffer");

    let primer: Vec<u8> = FILL_POISON
        .to_le_bytes()
        .iter()
        .copied()
        .cycle()
        .take(BYTES as usize)
        .collect();
    device
        .write_buffer(prime, 0, &primer)
        .expect("a host-upload buffer is what write_buffer is for");

    let barrier = |buffer, from, to| crcbl::hal::BufferBarrier {
        buffer,
        from,
        to,
        queue_transfer: None,
    };
    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("fill exercise"),
        queue: headless.queue,
    });
    encoder.pipeline_barrier(&Barriers {
        buffers: &[barrier(
            target,
            ResourceState::Undefined,
            ResourceState::TransferDst,
        )],
        ..Barriers::default()
    });
    encoder.copy_buffer_to_buffer(&crcbl::hal::BufferCopy {
        src: prime,
        src_offset: 0,
        dst: target,
        dst_offset: 0,
        size: BYTES,
    });
    encoder.pipeline_barrier(&Barriers {
        buffers: &[barrier(
            target,
            ResourceState::TransferDst,
            ResourceState::TransferDst,
        )],
        ..Barriers::default()
    });
    encoder.clear_buffer(target, 0, BYTES);
    encoder.pipeline_barrier(&Barriers {
        buffers: &[barrier(
            target,
            ResourceState::TransferDst,
            ResourceState::TransferSrc,
        )],
        ..Barriers::default()
    });
    encoder.copy_buffer_to_buffer(&crcbl::hal::BufferCopy {
        src: target,
        src_offset: 0,
        dst: staging,
        dst_offset: 0,
        size: BYTES,
    });

    let outcome = match encoder.finish() {
        Err(error) => Exercise::Refused(error),
        Ok(commands) => {
            device
                .submit(headless.queue, &SubmitInfo::new(&[commands]))
                .expect("submit");
            device.wait_idle().expect("idle");
            device.destroy_command_buffer(commands);

            let mut bytes = poisoned(BYTES as usize);
            headless.readback(staging, BYTES, &mut bytes);
            let words: Vec<u32> = bytes
                .chunks_exact(4)
                .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
                .collect();
            if words.iter().all(|word| *word == 0) {
                Exercise::Worked
            } else if words.iter().all(|word| *word == FILL_POISON) {
                Exercise::SilentlyIgnored
            } else {
                panic!(
                    "clear_buffer left {words:#010x?}, which is neither the zero it writes \
                     nor the {FILL_POISON:#010x} the destination was primed with — so the clear \
                     wrote something, and wrote the wrong thing"
                );
            }
        }
    };

    device.destroy_buffer(staging);
    device.destroy_buffer(target);
    device.destroy_buffer(prime);
    outcome
}

/// The colour format the image-to-image exercise moves.
///
/// `crcbl-webgpu`'s copy-chain probe — the browser-side copy of this same claim,
/// `probe_copychain_image_desc` — moves `Rgba8Unorm`, and this follows it rather
/// than picking a second format: it is the one colour format every API on this
/// seam stores as four tightly packed bytes with no reinterpretation, so a texel
/// that came back changed is the copy's doing and not a conversion's.
const IMAGE_COPY_FORMAT: Format = Format::Rgba8Unorm;

/// Texels across the two images the exercise copies between.
///
/// **256 is what picks this number**, exactly as it picks
/// [`DEPTH_COPY_WIDTH`]: the image is loaded and read back through buffer
/// copies, and WebGPU — with `wgpu` behind it, which is a backend this suite
/// runs — requires a buffer↔image copy's row pitch to be a multiple of 256
/// bytes. An [`IMAGE_COPY_FORMAT`] row this wide is exactly 256 bytes.
const IMAGE_COPY_WIDTH: u32 = 64;

/// Rows the exercise copies.
///
/// More than one, so an image-to-image copy that moved a single row, or that
/// read one image at the other's pitch, fails instead of passing on row 0.
const IMAGE_COPY_HEIGHT: u32 = 4;

/// The texel the destination image is primed with before the image-to-image
/// copy.
///
/// [`FILL_POISON`]'s job for a texture: a destination that still holds this
/// after `copy_image_to_image` proves the call was accepted and dropped rather
/// than performed, which is the [`Exercise::SilentlyIgnored`] the seam's rule
/// against a quiet no-op exists to catch. Distinct from [`POISON`] as well as
/// from the pattern, so "the destination was never overwritten" stays separable
/// from "nothing reached the readback at all".
const IMAGE_COPY_POISON: u32 = 0x3C3C_3C3C;

/// Drives [`Capability::ImageToImageCopy`] and reports whether the destination
/// image ended up holding what the source held.
///
/// # Why this creates its own images
///
/// The fixture's images are the swapchain's, which this suite does not own and
/// cannot make a copy destination. Two `TRANSFER_SRC | TRANSFER_DST` images
/// created here are the whole fixture the capability needs — nothing renders
/// into either, so this asks about
/// [`copy_image_to_image`](crcbl::hal::CommandEncoder::copy_image_to_image)
/// alone rather than about a pass.
///
/// # What is asserted, and why a backend that does nothing cannot pass it
///
/// The shape is `crcbl-webgpu`'s copy-chain probe (`probe.rs`'s
/// `probe_copychain_*`, gated in the browser as group V of
/// `web/tools/probe-groups.mjs`): a pattern goes into a source image, the source
/// is copied to a destination image, and the destination is copied back out to a
/// buffer and read. The browser's copy relies on a fresh WebGPU texture being
/// zero-initialised to tell a dropped copy from a performed one; nothing on this
/// seam promises that, so the destination is **primed** with
/// [`IMAGE_COPY_POISON`] through a buffer→image copy of its own first. That is
/// what makes the three outcomes separable, and every one of them is closed:
///
/// * the pattern in the readback means both copies ran — [`Exercise::Worked`];
/// * [`IMAGE_COPY_POISON`] means the prime landed and the image-to-image copy
///   was accepted and dropped — [`Exercise::SilentlyIgnored`];
/// * an error out of [`finish`](crcbl::hal::CommandEncoder::finish) means it was
///   refused. No backend answers that way today — `crcbl-dx12` did until it
///   implemented the copy — but the arm stays because a refusal deferred to
///   `finish` is how this seam's encoders report one, and a backend that gains
///   the capability later must not be the reason this exercise stops handling
///   it.
///
/// Every texel of the pattern differs from every other, so a copy that moved
/// row 0 into every row, or that read either image at the wrong pitch, comes
/// back as values that are neither the pattern nor the poison and panics rather
/// than passing.
fn exercise_image_to_image_copy(headless: &Headless) -> Exercise {
    let device = headless.device.as_ref();
    let texels = (IMAGE_COPY_WIDTH * IMAGE_COPY_HEIGHT) as usize;
    let bytes = (texels * 4) as u64;

    // An odd multiplier, so no two of these texels are equal: a repeated row and
    // a shifted pitch are both visible in the comparison rather than folded into
    // it.
    let pattern: Vec<u8> = (0..texels)
        .flat_map(|index| ((index as u32).wrapping_mul(2_654_435_761) ^ 0x5AA5_0FF0).to_le_bytes())
        .collect();
    // The premise the SilentlyIgnored arm rests on. A pattern texel that
    // happened to equal the poison would make a dropped copy read back as a
    // performed one for that texel, which is the exact confusion the priming
    // exists to remove.
    assert!(
        pattern
            .chunks_exact(4)
            .all(|texel| texel != IMAGE_COPY_POISON.to_le_bytes()),
        "a texel of this exercise's pattern equals IMAGE_COPY_POISON ({IMAGE_COPY_POISON:#010x}), \
         so a copy that was dropped and one that ran are no longer distinguishable"
    );
    let primer: Vec<u8> = IMAGE_COPY_POISON
        .to_le_bytes()
        .iter()
        .copied()
        .cycle()
        .take(bytes as usize)
        .collect();

    let host_buffer = |label, usage, memory| BufferDesc {
        label: Some(label),
        size: bytes,
        usage,
        memory,
    };
    let upload = device
        .create_buffer(&host_buffer(
            "image copy upload",
            BufferUsage::TRANSFER_SRC,
            MemoryLocation::HostUpload,
        ))
        .expect("a host-upload buffer");
    let prime = device
        .create_buffer(&host_buffer(
            "image copy prime",
            BufferUsage::TRANSFER_SRC,
            MemoryLocation::HostUpload,
        ))
        .expect("a host-upload buffer");
    let staging = device
        .create_buffer(&host_buffer(
            "image copy staging",
            BufferUsage::TRANSFER_DST,
            MemoryLocation::HostReadback,
        ))
        .expect("a host-readback buffer");
    device
        .write_buffer(upload, 0, &pattern)
        .expect("a host-upload buffer is what write_buffer is for");
    device
        .write_buffer(prime, 0, &primer)
        .expect("a host-upload buffer is what write_buffer is for");

    let describe = |label| ImageDesc {
        label: Some(label),
        image_type: ImageType::D2,
        extent: Extent3d::d2(IMAGE_COPY_WIDTH, IMAGE_COPY_HEIGHT),
        format: IMAGE_COPY_FORMAT,
        mip_levels: 1,
        samples: 1,
        usage: ImageUsage::TRANSFER_SRC | ImageUsage::TRANSFER_DST,
    };
    let source = device
        .create_image(&describe("image copy source"))
        .expect("a transfer-only colour image");
    let destination = device
        .create_image(&describe("image copy destination"))
        .expect("a transfer-only colour image");

    let subresource = ImageSubresourceLayers {
        aspect: ImageAspect::COLOR,
        mip: 0,
        base_layer: 0,
        layer_count: 1,
    };
    // Whole-subresource, tightly packed and at offset zero, for the same reason
    // the depth exercise's copies are: the row pitch and the buffer offset both
    // carry alignments a partial copy would have to satisfy by hand.
    let buffer_copy = |buffer, image| BufferImageCopy {
        buffer,
        buffer_offset: 0,
        buffer_row_length: 0,
        buffer_image_height: 0,
        image,
        image_subresource: subresource,
        image_offset: Offset3d { x: 0, y: 0, z: 0 },
        image_extent: Extent3d::d2(IMAGE_COPY_WIDTH, IMAGE_COPY_HEIGHT),
    };
    let range = ImageSubresourceRange {
        aspect: ImageAspect::COLOR,
        base_mip: 0,
        mip_count: 1,
        base_layer: 0,
        layer_count: 1,
    };
    let barrier = |image, from, to| ImageBarrier::new(image, range, from, to);

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("image copy exercise"),
        queue: headless.queue,
    });
    encoder.pipeline_barrier(&Barriers {
        images: &[
            barrier(source, ResourceState::Undefined, ResourceState::TransferDst),
            barrier(
                destination,
                ResourceState::Undefined,
                ResourceState::TransferDst,
            ),
        ],
        ..Barriers::default()
    });
    encoder.copy_buffer_to_image(&buffer_copy(upload, source));
    encoder.copy_buffer_to_image(&buffer_copy(prime, destination));
    // The destination stays a transfer destination across its second write, so
    // this pair is the write-after-write barrier between the prime and the copy
    // under test — the same shape `exercise_fill` puts between its prime and its
    // fill.
    encoder.pipeline_barrier(&Barriers {
        images: &[
            barrier(
                source,
                ResourceState::TransferDst,
                ResourceState::TransferSrc,
            ),
            barrier(
                destination,
                ResourceState::TransferDst,
                ResourceState::TransferDst,
            ),
        ],
        ..Barriers::default()
    });
    encoder.copy_image_to_image(&ImageCopy {
        src: source,
        src_subresource: subresource,
        src_offset: Offset3d { x: 0, y: 0, z: 0 },
        dst: destination,
        dst_subresource: subresource,
        dst_offset: Offset3d { x: 0, y: 0, z: 0 },
        extent: Extent3d::d2(IMAGE_COPY_WIDTH, IMAGE_COPY_HEIGHT),
    });
    encoder.pipeline_barrier(&Barriers {
        images: &[barrier(
            destination,
            ResourceState::TransferDst,
            ResourceState::TransferSrc,
        )],
        ..Barriers::default()
    });
    encoder.copy_image_to_buffer(&buffer_copy(staging, destination));

    let outcome = match encoder.finish() {
        Err(error) => Exercise::Refused(error),
        Ok(commands) => {
            device
                .submit(headless.queue, &SubmitInfo::new(&[commands]))
                .expect("submit");
            device.wait_idle().expect("idle");
            device.destroy_command_buffer(commands);

            let mut read = poisoned(bytes as usize);
            headless.readback(staging, bytes, &mut read);
            if read == pattern {
                Exercise::Worked
            } else if read == primer {
                Exercise::SilentlyIgnored
            } else if read.iter().all(|byte| *byte == POISON) {
                // Not a capability verdict: the prime is a buffer→image copy and
                // the read is an image→buffer one, and every backend here
                // performs both. All-poison means neither reached the staging
                // buffer, so this exercise is broken rather than the
                // image-to-image copy refused.
                panic!(
                    "the staging buffer still holds {POISON:#04x} in all {bytes} bytes, so \
                     nothing was copied out of the destination image at all — neither the prime \
                     nor the readback landed, and this exercise proves nothing about \
                     copy_image_to_image either way"
                );
            } else {
                let wrong = read
                    .iter()
                    .zip(&pattern)
                    .position(|(got, want)| got != want)
                    .expect("the two differ, so some byte differs");
                panic!(
                    "the destination image came back changed: byte {wrong} of {bytes} is {:#04x} \
                     and the source held {:#04x}. It is neither the pattern nor the \
                     {IMAGE_COPY_POISON:#010x} the destination was primed with, so the copy moved \
                     something and moved the wrong thing — a pitch, an offset or a subresource \
                     one side read differently from the other.",
                    read[wrong], pattern[wrong],
                );
            }
        }
    };

    device.destroy_image(destination);
    device.destroy_image(source);
    device.destroy_buffer(staging);
    device.destroy_buffer(prime);
    device.destroy_buffer(upload);
    outcome
}

/// The format the depth-copy exercise moves a plane of.
///
/// **[`Format::D16Unorm`] and not the [`Format::D32Float`] a shadow atlas uses**,
/// because this exercise drives the copy in BOTH directions and `D32Float` has
/// only one: WebGPU — and `wgpu` with it, which is a backend this suite runs —
/// reads a float depth plane back to a buffer and has no defined layout to write
/// one from. `D16Unorm` is the depth format every API on this seam moves either
/// way, so a round trip through it asks the whole capability rather than half of
/// it. [`Capability::DepthImageCopy`]'s own documentation carries the table.
const DEPTH_COPY_FORMAT: Format = Format::D16Unorm;

/// Texels across the depth image the exercise round-trips.
///
/// **256 IS WHAT PICKS THIS NUMBER**, not a preference about test data. WebGPU
/// requires a buffer↔image copy's row pitch to be a multiple of 256 bytes and
/// [`BufferImageCopy::buffer_row_length`] is in *texels* with no padding field,
/// so a width whose natural row is not already a multiple of 256 has no legal
/// tightly-packed copy at all on two of this suite's backends. A
/// [`DEPTH_COPY_FORMAT`] row this wide is exactly 256 bytes.
const DEPTH_COPY_WIDTH: u32 = 128;

/// Rows the exercise round-trips.
///
/// More than one, so a copy that transposed its row pitch or moved a single row
/// fails. Every row holds different values — see [`exercise_depth_image_copy`] —
/// so a copy that wrote row 0 four times fails too.
const DEPTH_COPY_HEIGHT: u32 = 4;

/// Drives [`Capability::DepthImageCopy`] in **both** directions and reports
/// whether the depth plane survived the round trip.
///
/// # What is asserted, and why a backend that does nothing cannot pass it
///
/// A pattern is uploaded into the depth plane with
/// [`copy_buffer_to_image`](crcbl::hal::CommandEncoder::copy_buffer_to_image),
/// copied straight back out with
/// [`copy_image_to_buffer`](crcbl::hal::CommandEncoder::copy_image_to_buffer),
/// and compared texel for texel. Every texel differs from every other, so the
/// three ways this passes while being broken are each closed: a copy that moved
/// nothing leaves [`POISON`] in the destination and reads back as
/// [`Exercise::SilentlyIgnored`]; a copy that moved row 0 into every row, or
/// that read the image back at the wrong pitch, reads back values that are not
/// the ones sent. The upload is what makes the readback mean anything — a
/// readback of an image nobody wrote is a comparison against undefined contents,
/// which is a test that passes on a backend that returns whatever was in memory.
///
/// **The image is never an attachment.** It is created `TRANSFER_SRC |
/// TRANSFER_DST` and nothing renders into it, so this asks about the copy alone
/// rather than about a depth pass — which is `tests/forward_e2e/shadow.rs`'s
/// question and needs a pipeline this suite does not build.
fn exercise_depth_image_copy(headless: &Headless) -> Exercise {
    let device = headless.device.as_ref();
    let texels = (DEPTH_COPY_WIDTH * DEPTH_COPY_HEIGHT) as usize;
    let bytes = (texels * 2) as u64;

    // Every texel its own value, and the two bytes of each differ from one
    // another: a swapped byte pair, a repeated row and a shifted pitch are all
    // visible in the comparison rather than folded into it.
    let pattern: Vec<u8> = (0..texels)
        .flat_map(|index| {
            let value = (index as u16).wrapping_mul(2749) ^ 0x5AA5;
            value.to_le_bytes()
        })
        .collect();

    let upload = device
        .create_buffer(&BufferDesc {
            label: Some("depth copy upload"),
            size: bytes,
            usage: BufferUsage::TRANSFER_SRC,
            memory: MemoryLocation::HostUpload,
        })
        .expect("a host-upload buffer");
    let staging = device
        .create_buffer(&BufferDesc {
            label: Some("depth copy staging"),
            size: bytes,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a host-readback buffer");
    let image = device
        .create_image(&ImageDesc {
            label: Some("depth copy image"),
            image_type: ImageType::D2,
            extent: Extent3d::d2(DEPTH_COPY_WIDTH, DEPTH_COPY_HEIGHT),
            format: DEPTH_COPY_FORMAT,
            mip_levels: 1,
            samples: 1,
            usage: ImageUsage::TRANSFER_SRC | ImageUsage::TRANSFER_DST,
        })
        .expect("a transfer-only depth image");
    device
        .write_buffer(upload, 0, &pattern)
        .expect("a host-upload buffer is what write_buffer is for");

    let subresource = ImageSubresourceLayers {
        aspect: ImageAspect::DEPTH,
        mip: 0,
        base_layer: 0,
        layer_count: 1,
    };
    // Whole-subresource, tightly packed and at offset zero. None of the three is
    // decoration: WebGPU permits no partial copy of a depth-stencil image at
    // all, and the row pitch and buffer offset both have alignments a partial
    // one would have to satisfy by hand.
    let copy = |buffer| BufferImageCopy {
        buffer,
        buffer_offset: 0,
        buffer_row_length: 0,
        buffer_image_height: 0,
        image,
        image_subresource: subresource,
        image_offset: Offset3d { x: 0, y: 0, z: 0 },
        image_extent: Extent3d::d2(DEPTH_COPY_WIDTH, DEPTH_COPY_HEIGHT),
    };
    let range = ImageSubresourceRange {
        aspect: ImageAspect::DEPTH,
        base_mip: 0,
        mip_count: 1,
        base_layer: 0,
        layer_count: 1,
    };

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("depth copy exercise"),
        queue: headless.queue,
    });
    encoder.pipeline_barrier(&Barriers {
        images: &[ImageBarrier::new(
            image,
            range,
            ResourceState::Undefined,
            ResourceState::TransferDst,
        )],
        ..Barriers::default()
    });
    encoder.copy_buffer_to_image(&copy(upload));
    encoder.pipeline_barrier(&Barriers {
        images: &[ImageBarrier::new(
            image,
            range,
            ResourceState::TransferDst,
            ResourceState::TransferSrc,
        )],
        ..Barriers::default()
    });
    encoder.copy_image_to_buffer(&copy(staging));

    let outcome = match encoder.finish() {
        Err(error) => Exercise::Refused(error),
        Ok(commands) => {
            device
                .submit(headless.queue, &SubmitInfo::new(&[commands]))
                .expect("submit");
            device.wait_idle().expect("idle");
            device.destroy_command_buffer(commands);

            let mut read = poisoned(bytes as usize);
            headless.readback(staging, bytes, &mut read);
            if read == pattern {
                Exercise::Worked
            } else if read.iter().all(|byte| *byte == POISON) {
                Exercise::SilentlyIgnored
            } else {
                let wrong = read
                    .iter()
                    .zip(&pattern)
                    .position(|(got, want)| got != want)
                    .expect("the two differ, so some byte differs");
                panic!(
                    "the depth plane came back changed: byte {wrong} of {bytes} is {:#04x} and \
                     {:#04x} was written. Neither direction of this copy is allowed to \
                     reinterpret a depth texel — it is a blit of the plane's own bytes — so this \
                     is a pitch, an aspect or a format the backend read differently from the one \
                     it wrote.",
                    read[wrong], pattern[wrong],
                );
            }
        }
    };

    device.destroy_image(image);
    device.destroy_buffer(staging);
    device.destroy_buffer(upload);
    outcome
}

/// The format the MSAA resolve exercise clears, resolves and reads back.
///
/// **The sRGB one, deliberately.** This exercise owns both its images, so it
/// names a format rather than taking the ring's — and naming the sRGB spelling
/// is what lets the resolved texels be compared against [`expected_level`],
/// which is the same assertion
/// [`a_render_pass_clear_reaches_memory_with_the_colour_it_was_given`] makes
/// about a clear that was never multisampled. A resolve is free of the usual
/// question about *where* an sRGB average happens, because every sample of this
/// attachment holds one value: the mean of four identical samples is that
/// sample whether the hardware averages encoded levels or decoded light.
const MSAA_RESOLVE_FORMAT: Format = Format::Rgba8UnormSrgb;

/// Samples the multisampled colour target carries.
///
/// Four, which is the multisample level every backend on this seam reports —
/// `crcbl-wgpu` answers a flat `max_sample_count: 4`, and `crcbl-vk` intersects
/// the three `VkPhysicalDeviceLimits` sample masks that matter. The exercise
/// reads [`crcbl::hal::Limits::max_sample_count`] rather than assuming it, and
/// says so instead of running, on a device that reports less.
const MSAA_RESOLVE_SAMPLES: u32 = 4;

/// Texels across the two targets the exercise resolves between.
///
/// **256 is what picks this number**, exactly as it picks [`IMAGE_COPY_WIDTH`]
/// and [`DEPTH_COPY_WIDTH`]: the resolve target is read back through a buffer
/// copy, and WebGPU — with `wgpu` behind it, which is a backend this suite runs
/// — requires such a copy's row pitch to be a multiple of 256 bytes. An
/// [`MSAA_RESOLVE_FORMAT`] row this wide is exactly 256 bytes.
const MSAA_RESOLVE_WIDTH: u32 = 64;

/// Rows the exercise resolves.
///
/// More than one, so a resolve that wrote a single row, or that walked the
/// destination at the multisampled target's pitch rather than the resolve
/// target's, fails instead of passing on row 0.
const MSAA_RESOLVE_HEIGHT: u32 = 4;

/// The byte every channel of the resolve target holds before the pass.
///
/// [`FILL_POISON`]'s job for this exercise's destination: a resolve target that
/// still holds this after the pass proves the resolve was accepted and dropped
/// rather than performed, which is the [`Exercise::SilentlyIgnored`] the seam's
/// rule against a quiet no-op exists to catch. Distinct from [`POISON`] too, so
/// "the resolve never wrote the target" stays separable from "nothing reached
/// the readback at all"; the exercise asserts it is also far enough from every
/// level [`CLEAR`] encodes to, which is what the `SilentlyIgnored` arm rests on.
const MSAA_RESOLVE_POISON: u8 = 0x69;

/// Why [`exercise_msaa_resolve`] declines a device whose multisample level is
/// below [`MSAA_RESOLVE_SAMPLES`].
///
/// The count itself is printed beside this rather than written into it: a
/// `&'static str` cannot carry the number the device reported.
const NO_MULTISAMPLING: &str = "this device reports a max_sample_count below the 4x this exercise resolves, so there is no \
     multisampled colour target to resolve from — the line above says what it reported";

/// Drives [`Capability::MsaaResolveAttachment`] and reports whether the pass's
/// resolve reached the single-sampled target it named.
///
/// # Why this needs no pipeline
///
/// **A clear needs no pipeline.** A resolve is an *end-of-pass* operation over
/// whatever the samples hold, and [`LoadOp::Clear`] puts a known value in every
/// one of them without a draw. So the pass here has no contents at all: it
/// clears a 4x multisampled colour target to [`CLEAR`], names a single-sampled
/// [`ColorAttachment::resolve`] view, and ends. That is why this one was driven
/// while its render-pass neighbour [`Capability::StencilReference`] was not,
/// until [`Raster`] gave the suite a pipeline to draw through.
///
/// # What is asserted, and why a backend that does nothing cannot pass it
///
/// This was `crcbl-wgpu`'s `an_msaa_pass_resolves_into_its_resolve_target` —
/// the one place in the workspace that attached a resolve view to a real
/// device, in a crate scheduled for deletion — brought to the seam and made
/// stricter in the two ways this suite's other exercises are:
///
/// * the resolve target is **primed** with [`MSAA_RESOLVE_POISON`] through a
///   buffer→image copy first, because a fresh image's contents are undefined on
///   this seam and a backend that dropped the resolve would otherwise be read
///   as one that performed it. The poison surviving is
///   [`Exercise::SilentlyIgnored`], which is exactly the bug that shipped in
///   `crcbl-wgpu` once: every `wgpu::RenderPassColorAttachment` was built with
///   `resolve_target: None`, so the pass rendered and nothing was ever
///   resolved — no error, no log, a wrong picture.
/// * the texels are compared against [`expected_level`] rather than against an
///   ordering. `red < green < blue` holds for [`CLEAR`] both encoded and
///   unencoded, so it cannot tell a target that skipped the sRGB encode from
///   one that applied it; the levels can, and they are 34 to 73 apart.
///
/// An error out of [`finish`](crcbl::hal::CommandEncoder::finish) is the third
/// outcome. No backend answers that way now — `crcbl-dx12` did until it learnt
/// to issue the `ResolveSubresource` after the pass that D3D12 wants instead of
/// a field on the attachment — but the arm stays, because a refusal deferred to
/// `finish` is how this seam's encoders report one.
///
/// # The multisampled *view* is what nothing had asked for before
///
/// [`ImageViewType`] has one D2 spelling and no multisampled one, so a backend
/// has to read the sample count off the image: `crcbl-dx12`'s `view::render_target`
/// does, choosing `D3D12_RTV_DIMENSION_TEXTURE2DMS`, and `crcbl-mtl`'s
/// `conv::view_texture_type` does not — every `ImageViewType::D2` becomes
/// `MTLTextureType::Type2D`, which is not among the types Metal will make a view
/// of a `Type2DMultisample` texture as. The `crcbl-wgpu` test this came from was
/// the only place in the workspace that had ever created a multisampled view at
/// all, and it ran on one backend — so this exercise is the first call to ask
/// `crcbl-mtl` and `crcbl-dx12` for one.
fn exercise_msaa_resolve(headless: &Headless) -> Exercise {
    let device = headless.device.as_ref();
    let available = device.caps().limits.max_sample_count;
    if available < MSAA_RESOLVE_SAMPLES {
        eprintln!(
            "crcbl hal seam e2e: max_sample_count is {available}, below the \
             {MSAA_RESOLVE_SAMPLES} this exercise resolves"
        );
        return Exercise::Unexercised(NO_MULTISAMPLING);
    }

    let extent = Extent3d::d2(MSAA_RESOLVE_WIDTH, MSAA_RESOLVE_HEIGHT);
    let bytes = u64::from(MSAA_RESOLVE_WIDTH) * u64::from(MSAA_RESOLVE_HEIGHT) * 4;

    // The multisampled target first, and its failure is the capability's own
    // rather than a broken fixture: a backend that cannot create a multisampled
    // colour image has nothing for a resolve view to resolve *from*.
    let msaa = match device.create_image(&ImageDesc {
        label: Some("msaa resolve source"),
        image_type: ImageType::D2,
        extent,
        format: MSAA_RESOLVE_FORMAT,
        mip_levels: 1,
        samples: MSAA_RESOLVE_SAMPLES,
        // Attachment only: nothing copies this image, and a multisampled
        // transfer source is a usage WebGPU does not have at all.
        usage: ImageUsage::COLOR_ATTACHMENT,
    }) {
        Ok(image) => image,
        Err(error) => return Exercise::Refused(error),
    };
    let resolved = device
        .create_image(&ImageDesc {
            label: Some("msaa resolve target"),
            image_type: ImageType::D2,
            extent,
            format: MSAA_RESOLVE_FORMAT,
            mip_levels: 1,
            samples: 1,
            // Written by the prime, resolved into by the pass, read back out.
            usage: ImageUsage::COLOR_ATTACHMENT
                | ImageUsage::TRANSFER_SRC
                | ImageUsage::TRANSFER_DST,
        })
        .expect("a single-sampled colour image");

    let view = |image, label| ImageViewDesc {
        label: Some(label),
        image,
        view_type: ImageViewType::D2,
        format: MSAA_RESOLVE_FORMAT,
        range: ImageSubresourceRange::all(MSAA_RESOLVE_FORMAT),
    };
    let msaa_view = device
        .create_image_view(&view(msaa, "msaa resolve source view"))
        .expect("a view of the multisampled target");
    let resolved_view = device
        .create_image_view(&view(resolved, "msaa resolve target view"))
        .expect("a view of the resolve target");

    let prime = device
        .create_buffer(&BufferDesc {
            label: Some("msaa resolve prime"),
            size: bytes,
            usage: BufferUsage::TRANSFER_SRC,
            memory: MemoryLocation::HostUpload,
        })
        .expect("a host-upload buffer");
    let staging = device
        .create_buffer(&BufferDesc {
            label: Some("msaa resolve staging"),
            size: bytes,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a host-readback buffer");

    let primer = vec![MSAA_RESOLVE_POISON; bytes as usize];
    device
        .write_buffer(prime, 0, &primer)
        .expect("a host-upload buffer is what write_buffer is for");

    // What the resolved texels must be: the clear, through the encode its
    // format owes it. Computed once, and asserted to be nothing like the poison
    // — a level that landed within tolerance of it would make a resolve that
    // was dropped and one that ran read back the same, which is the exact
    // confusion the priming exists to remove.
    let want = [
        expected_level(CLEAR[0], MSAA_RESOLVE_FORMAT),
        expected_level(CLEAR[1], MSAA_RESOLVE_FORMAT),
        expected_level(CLEAR[2], MSAA_RESOLVE_FORMAT),
        expected_level(CLEAR[3], MSAA_RESOLVE_FORMAT),
    ];
    assert!(
        want.iter()
            .all(|level| level.abs_diff(MSAA_RESOLVE_POISON) > LEVEL_TOLERANCE),
        "a channel of {CLEAR:?} encodes to {want:?}, within {LEVEL_TOLERANCE} of the \
         {MSAA_RESOLVE_POISON:#04x} the resolve target is primed with, so a resolve that was \
         dropped and one that ran are no longer distinguishable"
    );

    let range = ImageSubresourceRange::all(MSAA_RESOLVE_FORMAT);
    let barrier = |image, from, to| ImageBarrier::new(image, range, from, to);
    let subresource = ImageSubresourceLayers {
        aspect: ImageAspect::COLOR,
        mip: 0,
        base_layer: 0,
        layer_count: 1,
    };
    // Whole-subresource, tightly packed and at offset zero, for the reason
    // `MSAA_RESOLVE_WIDTH` gives: a partial copy carries row-pitch and
    // buffer-offset alignments that would have to be satisfied by hand.
    let buffer_copy = |buffer| BufferImageCopy {
        buffer,
        buffer_offset: 0,
        buffer_row_length: 0,
        buffer_image_height: 0,
        image: resolved,
        image_subresource: subresource,
        image_offset: Offset3d { x: 0, y: 0, z: 0 },
        image_extent: extent,
    };

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("msaa resolve exercise"),
        queue: headless.queue,
    });
    encoder.pipeline_barrier(&Barriers {
        images: &[
            barrier(
                msaa,
                ResourceState::Undefined,
                ResourceState::ColorAttachment,
            ),
            barrier(
                resolved,
                ResourceState::Undefined,
                ResourceState::TransferDst,
            ),
        ],
        ..Barriers::default()
    });
    encoder.copy_buffer_to_image(&buffer_copy(prime));
    // The resolve target is an attachment for the pass that writes it, so the
    // prime's write is ordered against the resolve by this transition — the
    // same shape `exercise_fill` puts between its prime and its fill, one
    // resource state along.
    encoder.pipeline_barrier(&Barriers {
        images: &[barrier(
            resolved,
            ResourceState::TransferDst,
            ResourceState::ColorAttachment,
        )],
        ..Barriers::default()
    });
    encoder.begin_render_pass(&RenderPassDesc {
        label: Some("msaa resolve clear"),
        color_attachments: &[ColorAttachment {
            view: msaa_view,
            resolve: Some(resolved_view),
            load: LoadOp::Clear,
            store: StoreOp::Store,
            clear: ClearValue::color(CLEAR),
        }],
        depth_stencil_attachment: None,
        render_area: Rect2d::from_size(MSAA_RESOLVE_WIDTH, MSAA_RESOLVE_HEIGHT),
        timestamp_writes: None,
    });
    encoder.end_render_pass();
    encoder.pipeline_barrier(&Barriers {
        images: &[barrier(
            resolved,
            ResourceState::ColorAttachment,
            ResourceState::TransferSrc,
        )],
        ..Barriers::default()
    });
    encoder.copy_image_to_buffer(&buffer_copy(staging));

    let outcome = match encoder.finish() {
        Err(error) => Exercise::Refused(error),
        Ok(commands) => {
            device
                .submit(headless.queue, &SubmitInfo::new(&[commands]))
                .expect("submit");
            device.wait_idle().expect("idle");
            device.destroy_command_buffer(commands);

            let mut read = poisoned(bytes as usize);
            headless.readback(staging, bytes, &mut read);
            let off_by = |texel: &[u8]| {
                texel
                    .iter()
                    .zip(&want)
                    .any(|(got, expected)| got.abs_diff(*expected) > LEVEL_TOLERANCE)
            };
            if read == primer {
                Exercise::SilentlyIgnored
            } else if read.iter().all(|byte| *byte == POISON) {
                // Not a capability verdict: the prime is a buffer→image copy
                // and the read an image→buffer one, and every backend that gets
                // this far performs both. All-poison means neither reached the
                // staging buffer, so this exercise is broken rather than the
                // resolve dropped.
                panic!(
                    "the staging buffer still holds {POISON:#04x} in all {bytes} bytes, so nothing \
                     was copied out of the resolve target at all — neither the prime nor the \
                     readback landed, and this exercise proves nothing about the resolve either way"
                );
            } else if let Some((index, texel)) = read
                .chunks_exact(4)
                .enumerate()
                .find(|(_, texel)| off_by(texel))
            {
                panic!(
                    "texel {index} of the resolve target came back as {texel:?} and a \
                     {MSAA_RESOLVE_FORMAT:?} target holding the clear {CLEAR:?} must put {want:?} \
                     there (±{LEVEL_TOLERANCE}). It is neither that nor the \
                     {MSAA_RESOLVE_POISON:#04x} the target was primed with, so the resolve wrote \
                     something and wrote the wrong thing.\n  \
                     encoded, which is what an sRGB target owes: {enc:?}\n  \
                     unencoded, which is what the linear values are: {lin:?}\n  \
                     Landing on the unencoded row means the resolve or the clear went through a \
                     view in the linear counterpart of this format.",
                    enc = [
                        expected_level(CLEAR[0], Format::Rgba8UnormSrgb),
                        expected_level(CLEAR[1], Format::Rgba8UnormSrgb),
                        expected_level(CLEAR[2], Format::Rgba8UnormSrgb),
                    ],
                    lin = [
                        expected_level(CLEAR[0], Format::Rgba8Unorm),
                        expected_level(CLEAR[1], Format::Rgba8Unorm),
                        expected_level(CLEAR[2], Format::Rgba8Unorm),
                    ],
                );
            } else {
                eprintln!(
                    "crcbl hal seam e2e: the {MSAA_RESOLVE_SAMPLES}x clear resolved to \
                     {:?} in {MSAA_RESOLVE_FORMAT:?}, against {want:?}",
                    &read[0..4]
                );
                Exercise::Worked
            }
        }
    };

    device.destroy_buffer(staging);
    device.destroy_buffer(prime);
    device.destroy_image_view(resolved_view);
    device.destroy_image_view(msaa_view);
    device.destroy_image(resolved);
    device.destroy_image(msaa);
    outcome
}

// --- the raster path --------------------------------------------------------
//
// One fixture behind five capabilities: a colour target, a storage buffer of
// vertices, and `crcbl-shaders`' committed `triangle` artifact — the only raster
// shader this workspace ships for every one of this suite's targets, in SPIR-V,
// WGSL, MSL and DXIL. [`Raster`] builds that much once per exercise and each
// exercise adds a pipeline that varies exactly one piece of state, so the texel
// that comes back is evidence about that piece and nothing else.

/// The format every raster exercise draws into and reads back.
///
/// **Linear, and not the sRGB spelling the ring prefers.** The fragment stage
/// returns the vertex colour unchanged, so an [`Rgba8Unorm`](Format::Rgba8Unorm)
/// target holds `round(c * 255)` and the comparison against [`expected_level`]
/// is exact to a level. That is also what makes the comparison worth making: a
/// target a backend viewed in this format's sRGB counterpart would encode every
/// write, and each of the colours below lands 30 to 60 levels away encoded — far
/// outside [`LEVEL_TOLERANCE`].
const RASTER_FORMAT: Format = Format::Rgba8Unorm;

/// Texels across the colour target.
///
/// **256 is what picks this number**, exactly as it picks [`IMAGE_COPY_WIDTH`],
/// [`DEPTH_COPY_WIDTH`] and [`MSAA_RESOLVE_WIDTH`]: the target is read back
/// through an image→buffer copy, and WebGPU — with `wgpu` behind it, which is a
/// backend this suite runs — requires such a copy's row pitch to be a multiple
/// of 256 bytes. A [`RASTER_FORMAT`] row this wide is exactly 256 bytes.
const RASTER_WIDTH: u32 = 64;

/// Rows of the colour target.
///
/// Enough that [`RASTER_TRIANGLE`] covers a wide interior, so the centre texel
/// the exercises read sits many texels away from every edge — which is what lets
/// a wireframe be told from a fill by one texel rather than by a threshold.
const RASTER_HEIGHT: u32 = 64;

/// Bytes the colour target occupies, and so the size of every buffer that
/// primes or reads it.
const RASTER_BYTES: u64 = RASTER_WIDTH as u64 * RASTER_HEIGHT as u64 * 4;

/// The triangle every raster exercise draws, in clip space.
///
/// **Symmetric about `x = 0` and containing the origin with room to spare**, so
/// the target's centre texel is inside it whichever sign convention clip-space Y
/// carries — Vulkan's, which `crcbl-vk` flips back with a negative-height
/// viewport, or the +Y-up one every other backend has natively. This suite is
/// not the place a Y flip is caught (`tests/render_e2e.rs` and `crcbl-vk`'s
/// golden triangle are), and a sample that moved with the convention would make
/// every verdict below a claim about the convention instead of about the
/// capability.
const RASTER_TRIANGLE: [[f32; 2]; 3] = [[-0.8, -0.8], [0.8, -0.8], [0.0, 0.8]];

/// Vertices one triangle contributes to the storage buffer.
const RASTER_TRIANGLE_VERTICES: u32 = RASTER_TRIANGLE.len() as u32;

/// The clip-space depth of a triangle that is inside the volume.
///
/// Anywhere in `0..=1` would do; the middle is chosen so neither plane is near.
const RASTER_DEPTH: f32 = 0.5;

/// The clip-space depth of the triangle [`exercise_depth_clamp`] draws.
///
/// Past the far plane at `w = 1`, so the primitive is culled entirely by the
/// clip stage and survives only where depth clamping replaced that clip. Nothing
/// else about it differs from the triangles above.
const RASTER_DEPTH_BEYOND_FAR: f32 = 1.5;

/// What every raster pass clears its colour target to.
///
/// Three different channels and none of them a fixed point of the sRGB transfer
/// function, for [`CLEAR`]'s reasons. It is the "the pass ran and the draw did
/// not land" reading, which is what most of the [`Exercise::SilentlyIgnored`]
/// arms below rest on.
const RASTER_BACKGROUND: [f32; 4] = [0.1, 0.3, 0.5, 1.0];

/// The colour of the first triangle in the pair.
const RASTER_FIRST: [f32; 4] = [0.9, 0.2, 0.6, 1.0];

/// The colour of the second triangle in the pair.
///
/// Drawn after [`RASTER_FIRST`] wherever a draw covers both, so "the second
/// triangle was reached" is a colour rather than an absence.
const RASTER_SECOND: [f32; 4] = [0.2, 0.7, 0.35, 1.0];

/// The colour of the triangle at [`RASTER_DEPTH_BEYOND_FAR`].
const RASTER_BEYOND: [f32; 4] = [0.6, 0.15, 0.85, 1.0];

/// The byte every channel of the colour target holds before the pass.
///
/// [`MSAA_RESOLVE_POISON`]'s job for this fixture, and it answers a different
/// question from the four colours: they separate "which draw landed", this
/// separates "the render pass ran at all". Every pass here loads with
/// [`LoadOp::Clear`], so a texel that still holds this was never touched by a
/// pass — which is a broken fixture rather than a capability verdict, and
/// [`Raster::render_target`] panics on it rather than folding it into one.
const RASTER_POISON: u8 = 0x5B;

/// The value the stencil plane is cleared to, and the reference
/// [`exercise_stencil_reference`]'s surviving draw sets.
const STENCIL_CLEARED: u32 = 0x2A;

/// The reference the second draw sets, which the cleared plane must reject.
const STENCIL_MISS: u32 = 0x11;

// The two references must differ, or the exercise cannot separate a reference
// that was applied from one that was not…
const _: () = assert!(STENCIL_CLEARED != STENCIL_MISS);
// …and neither may be the value a pass opens holding, or a backend that dropped
// `set_stencil_reference` entirely would draw the same picture as one that
// honoured it.
const _: () = assert!(STENCIL_CLEARED != crcbl::hal::stencil::INITIAL_REFERENCE);
const _: () = assert!(STENCIL_MISS != crcbl::hal::stencil::INITIAL_REFERENCE);

/// Bits the stencil comparison reads: all of them.
const STENCIL_READ_MASK: u32 = 0xFF;

/// The depth-stencil formats [`StencilTarget::open`] tries, in the order it
/// tries them.
///
/// **Two, because no single one is a depth-stencil attachment everywhere, and
/// the seam has no way to ask which is.** [`Device::caps`] reports features and
/// numeric limits and no format table at all, so the only probe available is to
/// create the image and look at what came back.
///
/// **A successful `create_image` means a usable image**, for every format this
/// suite knows how to ask about.
///
/// The seam's contract has two acceptable answers and one that is not: return a
/// handle that works, or refuse. What a backend may **not** do is answer `Ok`
/// with a handle the device cannot serve, because a caller has no way to tell
/// that from the good case and finds out at view creation, at pipeline
/// creation, or not at all.
///
/// # This caught a real defect, and it was invisible to every other test
///
/// `crcbl-vk::create_image` never asked the device whether it served the
/// format. `vkCreateImage` is not required to fail for an unsupported
/// format/usage pair, and radv returns success for
/// [`Format::D24UnormS8Uint`] as a depth-stencil attachment while the
/// validation layer reports `VK_ERROR_FORMAT_NOT_SUPPORTED` from
/// `vkGetPhysicalDeviceImageFormatProperties2`. The suite's own raster fixture
/// passed on undefined behaviour once before anyone read the layer output — and
/// it passes *legitimately* now only because it tries
/// [`Format::D32FloatS8Uint`] first, which every native API serves. So the
/// order of that list was hiding the bug from every backend at once.
///
/// # Both error channels, and that is not belt and braces
///
/// A backend may refuse through the return value or through
/// [`Device::take_error`] — `crcbl-mtl` uses the first for this case and
/// `crcbl-wgpu` the second, routing a bad format to the uncaptured-error
/// handler and returning a live-looking handle anyway. Reading one channel
/// would score one of them wrong, which is why the raster fixture reads both
/// and why this does.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-hal-seam-e2e.sh"]
fn a_created_image_is_one_the_device_can_serve() {
    let headless = Headless::open();
    let device = headless.device.as_ref();
    assert_no_out_of_band_error(device);

    let mut served = 0usize;
    for format in STENCIL_FORMATS {
        let image = match device.create_image(&ImageDesc {
            label: Some("format contract probe"),
            image_type: ImageType::D2,
            extent: Extent3d::d2(RASTER_WIDTH, RASTER_HEIGHT),
            format,
            mip_levels: 1,
            samples: 1,
            usage: ImageUsage::DEPTH_STENCIL_ATTACHMENT,
        }) {
            Ok(image) => image,
            Err(error) => {
                eprintln!(
                    "crcbl hal seam e2e: {format:?} refused as a depth-stencil attachment — \
                     {error}"
                );
                // A refusal must not also leave an error pending: a caller that
                // took the `Err` and carried on would meet it on an unrelated
                // call and blame that one.
                assert!(
                    device.take_error().is_none(),
                    "{format:?} was refused by return value *and* left an error pending, so the \
                     next unrelated call would report it"
                );
                continue;
            }
        };

        // The claim under test. `create_image` said yes; the device gets to
        // disagree exactly once, here, and not later.
        if let Some(error) = device.take_error() {
            device.destroy_image(image);
            panic!(
                "create_image returned Ok for {format:?} as a depth-stencil attachment and the \
                 device then reported {error}. That is the answer a caller cannot act on: the \
                 handle looks live and the failure arrives at view creation, at pipeline \
                 creation, or not at all. Refuse it instead."
            );
        }

        // And a view over it, which is where Vulkan's VUIDs landed when
        // `create_image` had lied.
        let view = device
            .create_image_view(&ImageViewDesc {
                label: Some("format contract probe view"),
                image,
                view_type: ImageViewType::D2,
                format,
                range: ImageSubresourceRange::all(format),
            })
            .unwrap_or_else(|error| {
                panic!(
                    "create_image accepted {format:?} and create_image_view then refused it — \
                     {error}. The image is not one this device serves, and the refusal belongs \
                     at creation where a caller can still choose another format."
                )
            });
        assert_no_out_of_band_error(device);
        device.destroy_image_view(view);
        device.destroy_image(image);
        served += 1;
    }

    // **A combination no device serves, so the refusal path runs somewhere.**
    // Without this the accepting path is all that runs on a capable device —
    // WARP serves both depth formats above, so `crcbl-dx12`'s refusal was
    // untested on the first run of this test and dx12 could not be called clean.
    // A colour format as a depth-stencil attachment is the cheapest input no
    // implementation can honour.
    //
    // **It does not assert a refusal**, deliberately. The claim under test is
    // still "if `create_image` says yes, the image is usable" — a backend that
    // somehow served this would have to serve it *properly*, and that would be
    // a finding rather than a failure. What the case buys is that on every
    // backend which does refuse, the refusal path is exercised at all.
    let impossible = ImageDesc {
        label: Some("format contract probe, impossible"),
        image_type: ImageType::D2,
        extent: Extent3d::d2(RASTER_WIDTH, RASTER_HEIGHT),
        format: Format::Rgba8Unorm,
        mip_levels: 1,
        samples: 1,
        usage: ImageUsage::DEPTH_STENCIL_ATTACHMENT,
    };
    match device.create_image(&impossible) {
        Err(error) => {
            eprintln!(
                "crcbl hal seam e2e: a colour format as a depth-stencil attachment is refused — \
                 {error}"
            );
            assert!(
                device.take_error().is_none(),
                "the impossible combination was refused by return value *and* left an error \
                 pending, so the next unrelated call would report it"
            );
        }
        Ok(image) => {
            let pending = device.take_error();
            device.destroy_image(image);
            assert!(
                pending.is_none(),
                "create_image returned Ok for a colour format as a depth-stencil attachment and \
                 the device then reported {}. Refuse it instead — the handle looks live and the \
                 failure arrives somewhere a caller cannot attribute it.",
                pending.expect("just checked"),
            );
            eprintln!(
                "crcbl hal seam e2e: this backend serves a colour format as a depth-stencil \
                 attachment, which is worth knowing"
            );
        }
    }

    // **A run where every format was refused proves nothing about the accepting
    // path**, and this suite draws that distinction everywhere else too.
    assert!(
        served > 0,
        "no format in {STENCIL_FORMATS:?} was served as a depth-stencil attachment, so this \
         checked only the refusal path — a device that can render depth at all serves one of them"
    );
    eprintln!(
        "crcbl hal seam e2e: {served} of {} depth-stencil format(s) served, and every \
         accepted one was usable",
        STENCIL_FORMATS.len()
    );
    headless.finish();
}

/// [`Format::D32FloatS8Uint`] is first because it is the one every *native* API
/// has: radv reports `D24_UNORM_S8_UINT` with no
/// `DEPTH_STENCIL_ATTACHMENT` format feature at all, and Metal's
/// `isDepth24Stencil8PixelFormatSupported` answers no on Apple silicon.
/// [`Format::D24UnormS8Uint`] is second and exists for wgpu, which is the
/// mirror image: `Depth24PlusStencil8` is core there and `Depth32FloatStencil8`
/// is behind a feature `crcbl-wgpu` does not enable.
const STENCIL_FORMATS: [Format; 2] = [Format::D32FloatS8Uint, Format::D24UnormS8Uint];

/// Why [`exercise_stencil_reference`] declines a device that will make neither
/// of [`STENCIL_FORMATS`].
const NO_STENCIL_FORMAT: &str = "this device would not create an image in either of the two depth-stencil formats the seam \
     spells, so there is no stencil plane to compare a reference against";

/// Bytes one `draw_indirect` argument structure occupies.
///
/// `VkDrawIndirectCommand`, `D3D12_DRAW_ARGUMENTS`,
/// `MTLDrawPrimitivesIndirectArguments` and `wgpu::util::DrawIndirectArgs` are
/// the same four 32-bit words in the same order — vertex count, instance count,
/// first vertex, first instance — because a driver reads these bytes directly.
/// The seam does not restate the layout, so this is the one the backends share.
const DRAW_ARGS_TIGHT_STRIDE: u32 = 16;

/// The stride [`exercise_indirect_argument_padded_stride`] asks for.
///
/// Twice [`DRAW_ARGS_TIGHT_STRIDE`], so an implementation that read its own
/// packed structures lands on the decoy the exercise puts between the two real
/// arguments rather than on the second of them.
const DRAW_ARGS_PADDED_STRIDE: u32 = DRAW_ARGS_TIGHT_STRIDE * 2;

/// Draws both indirect exercises record, and so the count both need room for.
const RASTER_INDIRECT_DRAWS: u32 = 2;

/// Why an indirect exercise declines a device that cannot record more than one
/// draw from a single call.
///
/// The stride between two argument structures is unobservable with one of them,
/// and so is a count buffer that selects one out of one.
const NO_MULTI_DRAW_INDIRECT: &str = "the observable in both indirect exercises is which of two argument structures a draw read, \
     and this device reports neither MULTI_DRAW_INDIRECT nor a max_draw_indirect_count above one \
     — so a single call can only ever reach the first";

/// One `draw_indirect` argument structure, little-endian.
///
/// `first_vertex` is zero in every one of these and that is deliberate rather
/// than incidental: `triangle.slang`'s `SV_VertexID` compiles to
/// `gl_VertexIndex - gl_BaseVertex` in SPIR-V and to a bare
/// `@builtin(vertex_index)` in WGSL, so a non-zero first vertex indexes a
/// *different* vertex on Vulkan from the one it indexes on wgpu. Every exercise
/// here varies the vertex **count** instead, which every target agrees about.
fn raster_draw_args(vertex_count: u32) -> [u8; DRAW_ARGS_TIGHT_STRIDE as usize] {
    let mut out = [0u8; DRAW_ARGS_TIGHT_STRIDE as usize];
    for (slot, value) in [vertex_count, 1, 0, 0].iter().enumerate() {
        out[slot * 4..slot * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    out
}

/// The bytes of one [`RASTER_TRIANGLE`], in the layout
/// `triangle.slang`'s `StructuredBuffer<Vertex>` declares.
///
/// Built out of [`crcbl::shaders::triangle::Vertex`] rather than out of a local
/// struct, so the layout is the one the crate that owns the shader source
/// publishes and cannot drift from it.
fn raster_triangle_bytes(color: [f32; 4], z: f32) -> Vec<u8> {
    let bytes: Vec<u8> = RASTER_TRIANGLE
        .iter()
        .map(|[x, y]| crcbl::shaders::triangle::Vertex {
            position: [*x, *y, z, 1.0],
            color,
        })
        .flat_map(|vertex| {
            vertex
                .position
                .into_iter()
                .chain(vertex.color)
                .flat_map(f32::to_le_bytes)
                .collect::<Vec<u8>>()
        })
        .collect();
    assert_eq!(
        bytes.len(),
        RASTER_TRIANGLE.len() * crcbl::shaders::triangle::VERTEX_STRIDE,
        "the packed vertices are not the stride `triangle.slang` declares, so the vertex stage \
         would read a different struct from the one written"
    );
    bytes
}

/// A linear colour as the four bytes [`RASTER_FORMAT`] stores it in.
fn raster_levels(color: [f32; 4]) -> [u8; 4] {
    [
        expected_level(color[0], RASTER_FORMAT),
        expected_level(color[1], RASTER_FORMAT),
        expected_level(color[2], RASTER_FORMAT),
        expected_level(color[3], RASTER_FORMAT),
    ]
}

/// Whether `texel` is `want` to within [`LEVEL_TOLERANCE`] on every channel.
fn raster_texel_is(texel: &[u8], want: [u8; 4]) -> bool {
    texel
        .iter()
        .zip(&want)
        .all(|(got, expected)| got.abs_diff(*expected) <= LEVEL_TOLERANCE)
}

/// The texel at `(column, row)` of a whole colour target.
fn raster_texel(read: &[u8], column: u32, row: u32) -> [u8; 4] {
    let index = (row * RASTER_WIDTH + column) as usize * 4;
    read[index..index + 4].try_into().expect("four bytes")
}

/// Texels of a whole colour target that are not [`RASTER_BACKGROUND`].
///
/// The number that separates a wireframe from a fill without a threshold — a
/// wireframe leaves the centre clear and still covers texels — and both from a
/// draw that never happened.
fn raster_drawn(read: &[u8]) -> usize {
    let background = raster_levels(RASTER_BACKGROUND);
    read.chunks_exact(4)
        .filter(|texel| !raster_texel_is(texel, background))
        .count()
}

/// What one raster pass left in the colour target.
///
/// Two numbers rather than the whole image, because two are what the exercises
/// that draw [`RASTER_TRIANGLE`] actually ask: which colour reached the centre,
/// and how much of the target stopped being the background at all — see
/// [`raster_drawn`] for what the second one separates.
///
/// The one exercise that asks something else — [`exercise_push_constants_on_graphics`],
/// whose two draws deliberately cover *different* columns — reads
/// [`Raster::render_target`] directly rather than widening this.
#[derive(Debug)]
struct RasterRead {
    /// The texel at the centre of the target, which [`RASTER_TRIANGLE`] covers.
    centre: [u8; 4],
    /// Texels that are not [`RASTER_BACKGROUND`].
    drawn: usize,
}

impl RasterRead {
    /// Reads the two numbers out of a whole target.
    ///
    /// The two ways the fixture itself can be broken are checked by
    /// [`Raster::render_target`], which is the only thing that produces the
    /// bytes this takes.
    fn from_bytes(read: &[u8]) -> Self {
        assert_eq!(
            read.len(),
            RASTER_BYTES as usize,
            "the readback is not the whole colour target"
        );
        let centre = raster_texel(read, RASTER_WIDTH / 2, RASTER_HEIGHT / 2);
        let drawn = raster_drawn(read);
        Self { centre, drawn }
    }

    /// Whether the centre texel holds `color`.
    fn is(&self, color: [f32; 4]) -> bool {
        raster_texel_is(&self.centre, raster_levels(color))
    }

    /// The centre texel and the levels each known colour would have put there,
    /// for a panic that has to say what the target came back holding.
    fn diagnosis(&self) -> String {
        format!(
            "the centre texel is {:?} and {} of {} texels are not the background.\n  \
             background {:?}\n  first {:?}\n  second {:?}\n  beyond the far plane {:?}",
            self.centre,
            self.drawn,
            RASTER_WIDTH * RASTER_HEIGHT,
            raster_levels(RASTER_BACKGROUND),
            raster_levels(RASTER_FIRST),
            raster_levels(RASTER_SECOND),
            raster_levels(RASTER_BEYOND),
        )
    }
}

/// A depth-stencil attachment one exercise creates for itself.
struct StencilTarget {
    image: crcbl::hal::ImageHandle,
    view: crcbl::hal::ImageViewHandle,
    format: Format,
}

impl StencilTarget {
    /// The first of [`STENCIL_FORMATS`] this device will make a depth-stencil
    /// attachment of, or `None` when it will make neither.
    ///
    /// # Both error channels, because the backends use both
    ///
    /// A refusal arrives as a returned [`HalError`] on the native backends —
    /// `crcbl-mtl` answers [`HalError::InvalidDescriptor`] for `D24UnormS8Uint`
    /// on Apple silicon — and out of band on wgpu, where `create_texture`
    /// reports a format the device did not enable through the uncaptured-error
    /// handler and hands back a texture regardless. Reading only the return
    /// value would take that texture and build a pass on it; reading only
    /// [`Device::take_error`] would miss Metal's. So this checks both, and
    /// [`assert_no_out_of_band_error`] runs *first* so that an error already
    /// pending is reported rather than swallowed by the probe.
    fn open(device: &dyn Device) -> Option<Self> {
        assert_no_out_of_band_error(device);
        for format in STENCIL_FORMATS {
            let Ok(image) = device.create_image(&ImageDesc {
                label: Some("raster stencil target"),
                image_type: ImageType::D2,
                extent: Extent3d::d2(RASTER_WIDTH, RASTER_HEIGHT),
                format,
                mip_levels: 1,
                samples: 1,
                usage: ImageUsage::DEPTH_STENCIL_ATTACHMENT,
            }) else {
                continue;
            };
            if let Some(error) = device.take_error() {
                eprintln!(
                    "crcbl hal seam e2e: {format:?} is not a depth-stencil attachment on this \
                     device — {error}"
                );
                device.destroy_image(image);
                continue;
            }
            let view = device
                .create_image_view(&ImageViewDesc {
                    label: Some("raster stencil target view"),
                    image,
                    view_type: ImageViewType::D2,
                    format,
                    range: ImageSubresourceRange::all(format),
                })
                .expect("a view of an image this device has just created");
            eprintln!("crcbl hal seam e2e: the stencil exercise runs against {format:?}");
            return Some(Self {
                image,
                view,
                format,
            });
        }
        None
    }

    fn destroy(self, device: &dyn Device) {
        device.destroy_image_view(self.view);
        device.destroy_image(self.image);
    }
}

/// A colour target, the geometry the committed `triangle` artifact pulls, and
/// the layout every raster pipeline below is built against.
struct Raster {
    color: crcbl::hal::ImageHandle,
    color_view: crcbl::hal::ImageViewHandle,
    /// A host buffer of nothing but [`RASTER_POISON`], copied over the target
    /// before every pass.
    prime: crcbl::hal::BufferHandle,
    /// Host-readable copy target, so the pass's output can be asserted.
    staging: crcbl::hal::BufferHandle,
    /// [`RASTER_FIRST`] at vertices `0..3` and [`RASTER_SECOND`] at `3..6`, so
    /// a draw of three vertices and a draw of six land different colours in the
    /// same texels.
    pair: crcbl::hal::BufferHandle,
    pair_group: crcbl::hal::BindGroupHandle,
    /// [`RASTER_BEYOND`] at [`RASTER_DEPTH_BEYOND_FAR`], on its own buffer
    /// because every draw here starts at vertex zero.
    beyond: crcbl::hal::BufferHandle,
    beyond_group: crcbl::hal::BindGroupHandle,
    bind_group_layout: crcbl::hal::BindGroupLayoutHandle,
    pipeline_layout: crcbl::hal::PipelineLayoutHandle,
    /// Kept alive rather than destroyed at the first pipeline, because each
    /// exercise builds its own from it.
    module: crcbl::hal::ShaderModuleHandle,
}

impl Raster {
    /// Builds the target, the layout and the module, and stages the geometry in.
    fn open(headless: &Headless) -> Self {
        let device = headless.device.as_ref();

        // The premise every verdict below rests on: no two of these colours are
        // within the tolerance the texel comparison uses, so "which draw landed"
        // is answerable from one texel.
        let palette = [
            ("background", RASTER_BACKGROUND),
            ("first", RASTER_FIRST),
            ("second", RASTER_SECOND),
            ("beyond", RASTER_BEYOND),
        ];
        for (index, (name, color)) in palette.iter().enumerate() {
            for (other_name, other) in &palette[index + 1..] {
                assert!(
                    !raster_texel_is(&raster_levels(*color), raster_levels(*other)),
                    "the {name} and {other_name} colours land within {LEVEL_TOLERANCE} of each \
                     other in {RASTER_FORMAT:?} ({:?} against {:?}), so a texel cannot say which \
                     draw reached it",
                    raster_levels(*color),
                    raster_levels(*other),
                );
            }
            assert!(
                !raster_texel_is(&raster_levels(*color), [RASTER_POISON; 4]),
                "the {name} colour lands within {LEVEL_TOLERANCE} of the {RASTER_POISON:#04x} the \
                 target is primed with, so a pass that never ran and one that drew are no longer \
                 distinguishable"
            );
        }

        let color = device
            .create_image(&ImageDesc {
                label: Some("raster colour target"),
                image_type: ImageType::D2,
                extent: Extent3d::d2(RASTER_WIDTH, RASTER_HEIGHT),
                format: RASTER_FORMAT,
                mip_levels: 1,
                samples: 1,
                // Primed by a copy, drawn into by the pass, read back out.
                usage: ImageUsage::COLOR_ATTACHMENT
                    | ImageUsage::TRANSFER_SRC
                    | ImageUsage::TRANSFER_DST,
            })
            .expect("a single-sampled colour image");
        let color_view = device
            .create_image_view(&ImageViewDesc {
                label: Some("raster colour target view"),
                image: color,
                view_type: ImageViewType::D2,
                format: RASTER_FORMAT,
                range: ImageSubresourceRange::all(RASTER_FORMAT),
            })
            .expect("a view of the colour target");

        let prime = device
            .create_buffer(&BufferDesc {
                label: Some("raster prime"),
                size: RASTER_BYTES,
                usage: BufferUsage::TRANSFER_SRC,
                memory: MemoryLocation::HostUpload,
            })
            .expect("a host-upload buffer");
        device
            .write_buffer(prime, 0, &vec![RASTER_POISON; RASTER_BYTES as usize])
            .expect("a host-upload buffer is what write_buffer is for");
        let staging = device
            .create_buffer(&BufferDesc {
                label: Some("raster readback"),
                size: RASTER_BYTES,
                usage: BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::HostReadback,
            })
            .expect("a host-readback buffer");

        let mut pair_bytes = raster_triangle_bytes(RASTER_FIRST, RASTER_DEPTH);
        pair_bytes.extend(raster_triangle_bytes(RASTER_SECOND, RASTER_DEPTH));
        let beyond_bytes = raster_triangle_bytes(RASTER_BEYOND, RASTER_DEPTH_BEYOND_FAR);

        let pair = raster_storage_buffer(headless, "raster pair vertices", &pair_bytes);
        let beyond = raster_storage_buffer(headless, "raster beyond vertices", &beyond_bytes);

        let layout_entries = [crcbl::hal::BindGroupLayoutEntry {
            binding: 0,
            visibility: crcbl::hal::ShaderStages::VERTEX,
            // `StructuredBuffer<Vertex>` rather than `RWStructuredBuffer`, which
            // is what makes this read-only on the Rust side too.
            kind: crcbl::hal::BindingKind::StorageBuffer {
                read_only: true,
                dynamic: false,
            },
            count: 1,
            flags: crcbl::hal::BindingFlags::empty(),
        }];
        let bind_group_layout = device
            .create_bind_group_layout(&crcbl::hal::BindGroupLayoutDesc {
                label: Some("raster vertices"),
                entries: &layout_entries,
            })
            .expect("a layout with no descriptor-indexing flags works on every tier");

        let group = |label, buffer| {
            let entries = [crcbl::hal::BindGroupEntry {
                binding: 0,
                array_index: 0,
                resource: crcbl::hal::BindingResource::whole_buffer(buffer),
            }];
            device
                .create_bind_group(&crcbl::hal::BindGroupDesc {
                    label: Some(label),
                    layout: bind_group_layout,
                    entries: &entries,
                    variable_count: None,
                })
                .expect("a bind group")
        };
        let pair_group = group("raster pair", pair);
        let beyond_group = group("raster beyond", beyond);

        let set_layouts = [bind_group_layout];
        let pipeline_layout = device
            .create_pipeline_layout(&crcbl::hal::PipelineLayoutDesc {
                label: Some("raster"),
                bind_group_layouts: &set_layouts,
                push_constants: None,
            })
            .expect("a pipeline layout");

        // All four blobs, because all four backends run this file — the same
        // reason `ComputeProbe::new` passes all four.
        let module = device
            .create_shader_module(&crcbl::hal::ShaderModuleDesc {
                label: Some("triangle.slang"),
                spirv: crcbl::shaders::TRIANGLE.spirv(),
                wgsl: crcbl::shaders::TRIANGLE.wgsl(),
                msl: crcbl::shaders::TRIANGLE.msl(),
                dxil: &crcbl::shaders::TRIANGLE.dxil_containers(),
            })
            .expect("the committed shader is accepted");

        Self {
            color,
            color_view,
            prime,
            staging,
            pair,
            pair_group,
            beyond,
            beyond_group,
            bind_group_layout,
            pipeline_layout,
            module,
        }
    }

    /// A pipeline over this fixture's layout and module, varying only the two
    /// pieces of state the exercises differ in.
    ///
    /// # Errors
    ///
    /// Whatever `create_graphics_pipeline` returned, which for
    /// [`PolygonMode::Line`] and [`PrimitiveState::depth_clamp`] on a device
    /// without the feature is the capability's refusal and is the point.
    fn pipeline(
        &self,
        device: &dyn Device,
        label: &'static str,
        primitive: PrimitiveState,
        depth_stencil: Option<crcbl::hal::DepthStencilState>,
    ) -> Result<crcbl::hal::GraphicsPipelineHandle, HalError> {
        raster_pipeline(
            device,
            label,
            self.pipeline_layout,
            self.module,
            &crcbl::shaders::TRIANGLE,
            primitive,
            depth_stencil,
        )
    }

    /// Primes the target, runs `record` inside one render pass, and reads the
    /// target back.
    ///
    /// `record` is the only thing that varies between the exercises, so all of
    /// them go through the same prime, the same clear and the same readback and
    /// a difference in the result is a difference in what was recorded.
    ///
    /// # Errors
    ///
    /// [`render_target`](Self::render_target)'s.
    fn render(
        &self,
        headless: &Headless,
        label: &'static str,
        depth: Option<&StencilTarget>,
        record: impl FnOnce(&mut dyn crcbl::hal::CommandEncoder),
    ) -> Result<RasterRead, HalError> {
        let read = self.render_target(headless, label, depth, record)?;
        Ok(RasterRead::from_bytes(&read))
    }

    /// [`render`](Self::render) without the reading: the whole colour target, so
    /// an exercise whose observable is not [`RasterRead`]'s two numbers can ask
    /// its own question of it.
    ///
    /// The two ways the *fixture* can be broken are checked here rather than in
    /// either reader, because they are facts about the pass rather than about
    /// what was drawn in it — and a second reader that forgot to check them
    /// would report a pass that never happened as a capability verdict.
    ///
    /// # Errors
    ///
    /// Whatever `finish` returned. That is where a refusal this seam's encoders
    /// defer surfaces — `crcbl-wgpu`'s padded-stride refusal is recorded at the
    /// call and reported here — so the caller reads it as
    /// [`Exercise::Refused`] rather than losing it.
    fn render_target(
        &self,
        headless: &Headless,
        label: &'static str,
        depth: Option<&StencilTarget>,
        record: impl FnOnce(&mut dyn crcbl::hal::CommandEncoder),
    ) -> Result<Vec<u8>, HalError> {
        let device = headless.device.as_ref();
        let extent = Extent3d::d2(RASTER_WIDTH, RASTER_HEIGHT);
        let range = ImageSubresourceRange::all(RASTER_FORMAT);
        let subresource = ImageSubresourceLayers {
            aspect: ImageAspect::COLOR,
            mip: 0,
            base_layer: 0,
            layer_count: 1,
        };
        // Whole-subresource, tightly packed and at offset zero, for the reason
        // `RASTER_WIDTH` gives: a partial copy carries row-pitch and
        // buffer-offset alignments that would have to be satisfied by hand.
        let buffer_copy = |buffer| BufferImageCopy {
            buffer,
            buffer_offset: 0,
            buffer_row_length: 0,
            buffer_image_height: 0,
            image: self.color,
            image_subresource: subresource,
            image_offset: Offset3d { x: 0, y: 0, z: 0 },
            image_extent: extent,
        };

        let mut entering = vec![ImageBarrier::new(
            self.color,
            range,
            ResourceState::Undefined,
            ResourceState::TransferDst,
        )];
        if let Some(target) = depth {
            entering.push(ImageBarrier::new(
                target.image,
                ImageSubresourceRange::all(target.format),
                ResourceState::Undefined,
                ResourceState::DepthStencilWrite,
            ));
        }

        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some(label),
            queue: headless.queue,
        });
        encoder.pipeline_barrier(&Barriers {
            images: &entering,
            ..Barriers::default()
        });
        encoder.copy_buffer_to_image(&buffer_copy(self.prime));
        // The target is an attachment for the pass that draws into it, so the
        // prime's write is ordered against the clear by this transition — the
        // same shape `exercise_msaa_resolve` puts between its prime and its
        // pass.
        encoder.pipeline_barrier(&Barriers {
            images: &[ImageBarrier::new(
                self.color,
                range,
                ResourceState::TransferDst,
                ResourceState::ColorAttachment,
            )],
            ..Barriers::default()
        });
        encoder.begin_render_pass(&RenderPassDesc {
            label: Some(label),
            color_attachments: &[ColorAttachment {
                view: self.color_view,
                resolve: None,
                load: LoadOp::Clear,
                store: StoreOp::Store,
                clear: ClearValue::color(RASTER_BACKGROUND),
            }],
            depth_stencil_attachment: depth.map(|target| DepthStencilAttachment {
                view: target.view,
                // The stencil load below writes the plane, so this attachment is
                // written rather than read and the barrier above matches.
                read_only: false,
                depth_load: LoadOp::Clear,
                depth_store: StoreOp::Discard,
                stencil_load: LoadOp::Clear,
                stencil_store: StoreOp::Discard,
                clear: ClearValue {
                    stencil: STENCIL_CLEARED,
                    ..ClearValue::default()
                },
            }),
            render_area: Rect2d::from_size(RASTER_WIDTH, RASTER_HEIGHT),
            timestamp_writes: None,
        });
        encoder.set_viewport(&crcbl::hal::Viewport::from_size(
            RASTER_WIDTH,
            RASTER_HEIGHT,
        ));
        encoder.set_scissor(&Rect2d::from_size(RASTER_WIDTH, RASTER_HEIGHT));
        record(encoder.as_mut());
        encoder.end_render_pass();
        encoder.pipeline_barrier(&Barriers {
            images: &[ImageBarrier::new(
                self.color,
                range,
                ResourceState::ColorAttachment,
                ResourceState::TransferSrc,
            )],
            ..Barriers::default()
        });
        encoder.copy_image_to_buffer(&buffer_copy(self.staging));

        let commands = encoder.finish()?;
        device
            .submit(headless.queue, &SubmitInfo::new(&[commands]))
            .expect("submit");
        device.wait_idle().expect("idle");
        device.destroy_command_buffer(commands);

        let mut read = poisoned(RASTER_BYTES as usize);
        headless.readback(self.staging, RASTER_BYTES, &mut read);
        assert!(
            !read.iter().all(|byte| *byte == POISON),
            "the staging buffer still holds {POISON:#04x} in all {RASTER_BYTES} bytes, so nothing \
             was copied out of the colour target at all — the readback never landed, and this \
             exercise proves nothing about what was drawn either way"
        );
        assert!(
            !read.iter().all(|byte| *byte == RASTER_POISON),
            "every texel still holds the {RASTER_POISON:#04x} the target was primed with, so the \
             render pass never ran: its LoadOp::Clear alone would have replaced this. Nothing \
             about the draw inside it can be read off a pass that did not happen."
        );
        Ok(read)
    }

    fn destroy(self, device: &dyn Device) {
        device.destroy_shader_module(self.module);
        device.destroy_pipeline_layout(self.pipeline_layout);
        device.destroy_bind_group(self.beyond_group);
        device.destroy_bind_group(self.pair_group);
        device.destroy_bind_group_layout(self.bind_group_layout);
        device.destroy_buffer(self.beyond);
        device.destroy_buffer(self.pair);
        device.destroy_buffer(self.staging);
        device.destroy_buffer(self.prime);
        device.destroy_image_view(self.color_view);
        device.destroy_image(self.color);
    }
}

/// A graphics pipeline against `layout` and `module`, over the raster fixture's
/// colour target.
///
/// Split out of [`Raster::pipeline`] when [`exercise_push_constants_on_graphics`]
/// needed one built over a *different* layout and module — a layout carrying a
/// push-constant range, and the artifact that reads it. What stays shared is the
/// part that describes the target rather than the exercise: the colour-target
/// state and the multisample state are [`Raster`]'s, and a second copy of them
/// is a second thing to change the day [`RASTER_FORMAT`] does.
///
/// # Errors
///
/// Whatever `create_graphics_pipeline` returned, which for
/// [`PolygonMode::Line`] and [`PrimitiveState::depth_clamp`] on a device without
/// the feature is the capability's refusal and is the point.
fn raster_pipeline(
    device: &dyn Device,
    label: &'static str,
    layout: crcbl::hal::PipelineLayoutHandle,
    module: crcbl::hal::ShaderModuleHandle,
    shader: &crcbl::shaders::Shader,
    primitive: PrimitiveState,
    depth_stencil: Option<crcbl::hal::DepthStencilState>,
) -> Result<crcbl::hal::GraphicsPipelineHandle, HalError> {
    let color_targets = [crcbl::hal::ColorTargetState::opaque(RASTER_FORMAT)];
    let entry = |stage| {
        crcbl::hal::ShaderEntry {
            module,
            // The manifest's name rather than a literal, for
            // `ComputeProbe::new`'s reason: it is read out of the artifact's own
            // entry-point table by the compile script.
            entry_point: shader
                .entry_point(stage)
                .expect("the artifact names both of its graphics stages"),
        }
    };
    device.create_graphics_pipeline(&crcbl::hal::GraphicsPipelineDesc {
        label: Some(label),
        layout,
        vertex: entry(crcbl::shaders::Stage::Vertex),
        fragment: Some(entry(crcbl::shaders::Stage::Fragment)),
        primitive,
        depth_stencil,
        multisample: crcbl::hal::MultisampleState::default(),
        color_targets: &color_targets,
    })
}

/// Stages `bytes` into a device-local storage buffer, on its own submission.
///
/// The upload is a copy from a host buffer rather than a write to the storage
/// buffer itself, which is what `TriangleResources::new` in `crcbl-vk`'s suite
/// does and for the same reason: a host-visible storage buffer is a memory type
/// wgpu will not hand out.
fn raster_storage_buffer(
    headless: &Headless,
    label: &'static str,
    bytes: &[u8],
) -> crcbl::hal::BufferHandle {
    staged_buffer(
        headless,
        label,
        bytes,
        BufferUsage::STORAGE,
        ResourceState::ShaderRead,
    )
}

/// Stages `bytes` into a device-local indirect-argument buffer, on its own
/// submission and left in [`ResourceState::IndirectArgument`].
fn raster_indirect_buffer(
    headless: &Headless,
    label: &'static str,
    bytes: &[u8],
) -> crcbl::hal::BufferHandle {
    staged_buffer(
        headless,
        label,
        bytes,
        BufferUsage::INDIRECT,
        ResourceState::IndirectArgument,
    )
}

/// A device-local buffer holding `bytes`: one host buffer, one copy, one
/// barrier, one submission that is waited on.
///
/// Its own submission rather than the caller's command buffer, so the
/// exercises' [`Raster::render`] records the same barriers whatever it is
/// drawing — a submission boundary is the dependency, so nothing is left
/// unsynchronised by moving the upload out. Written for the two raster helpers
/// above and named for neither of them, because
/// [`exercise_update_bind_group`]'s compute fixture stages its uniform and its
/// source through it too.
fn staged_buffer(
    headless: &Headless,
    label: &'static str,
    bytes: &[u8],
    usage: BufferUsage,
    state: ResourceState,
) -> crcbl::hal::BufferHandle {
    let device = headless.device.as_ref();
    let size = bytes.len() as u64;
    let upload = device
        .create_buffer(&BufferDesc {
            label: Some(label),
            size,
            usage: BufferUsage::TRANSFER_SRC,
            memory: MemoryLocation::HostUpload,
        })
        .expect("a host-upload buffer");
    device
        .write_buffer(upload, 0, bytes)
        .expect("a host-upload buffer is what write_buffer is for");
    let buffer = device
        .create_buffer(&BufferDesc {
            label: Some(label),
            size,
            usage: usage | BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("a device-local buffer");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some(label),
        queue: headless.queue,
    });
    encoder.copy_buffer_to_buffer(&crcbl::hal::BufferCopy {
        src: upload,
        src_offset: 0,
        dst: buffer,
        dst_offset: 0,
        size,
    });
    encoder.pipeline_barrier(&Barriers {
        buffers: &[crcbl::hal::BufferBarrier {
            buffer,
            from: ResourceState::TransferDst,
            to: state,
            queue_transfer: None,
        }],
        ..Barriers::default()
    });
    let commands = encoder.finish().expect("recording succeeded");
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");
    device.wait_idle().expect("idle");
    device.destroy_command_buffer(commands);
    device.destroy_buffer(upload);
    buffer
}

/// Drives [`Capability::StencilReference`] and reports whether the value
/// [`set_stencil_reference`](crcbl::hal::CommandEncoder::set_stencil_reference)
/// carried decided which draw survived — **including across a pipeline bind**.
///
/// # What is asserted, and why a backend that does nothing cannot pass it
///
/// The stencil plane is cleared to [`STENCIL_CLEARED`] and the pipeline compares
/// [`CompareOp::Equal`](crcbl::hal::CompareOp::Equal) against it, writing
/// nothing back. Two draws follow, in one pass, with the reference set
/// differently before each:
///
/// * [`STENCIL_CLEARED`], then the first triangle — which must pass the test and
///   put [`RASTER_FIRST`] in the centre;
/// * [`STENCIL_MISS`], then *both* triangles — which must fail it and leave that
///   texel alone.
///
/// So [`RASTER_FIRST`] is the only reading a backend that applied both values
/// produces, and every other way this can go lands somewhere else.
/// [`RASTER_SECOND`] means the second reference never took effect — either the
/// call was dropped or the test is not enabled at all, and the draw that should
/// have been discarded drew over the first. [`RASTER_BACKGROUND`] means the
/// *first* reference was not in force when the first draw ran, so both draws
/// were tested against something the plane does not hold. Both are
/// [`Exercise::SilentlyIgnored`]: the reference was accepted and changed nothing
/// observable.
///
/// # Two orderings carry the whole claim
///
/// **The first `set_stencil_reference` comes before
/// [`bind_graphics_pipeline`](crcbl::hal::CommandEncoder::bind_graphics_pipeline),
/// deliberately.** The seam states that binding a pipeline does not disturb the
/// current reference (see
/// [`StencilState`](crcbl::hal::StencilState)), and D3D12 and Metal both hold a
/// per-pipeline `OMSetStencilRef`/`setStencilReferenceValue:` that a backend is
/// free to re-apply at every bind — which is what they used to do, from a
/// pipeline field the seam has since dropped. With the call before the bind, a
/// backend that re-applies anything lands on [`RASTER_BACKGROUND`]; with it
/// after, the clobber is invisible and every backend passes.
///
/// **The matching reference draws first**, and that is not free to reverse
/// either: with the rejected reference first, "the stencil test is not enabled"
/// and "both references were applied" produce the same texel, because the last
/// draw would win either way.
fn exercise_stencil_reference(headless: &Headless) -> Exercise {
    let device = headless.device.as_ref();
    let Some(target) = StencilTarget::open(device) else {
        return Exercise::Unexercised(NO_STENCIL_FORMAT);
    };
    let raster = Raster::open(headless);

    let face = crcbl::hal::StencilFaceState {
        compare: crcbl::hal::CompareOp::Equal,
        fail_op: crcbl::hal::StencilOp::Keep,
        depth_fail_op: crcbl::hal::StencilOp::Keep,
        pass_op: crcbl::hal::StencilOp::Keep,
    };
    let depth_stencil = crcbl::hal::DepthStencilState {
        format: target.format,
        // The depth half is deliberately inert: this exercise is about the
        // stencil reference, and a depth test that could also discard a fragment
        // would be a second explanation for an empty target.
        depth_write: false,
        depth_compare: crcbl::hal::CompareOp::Always,
        stencil: Some(crcbl::hal::StencilState {
            front: face,
            back: face,
            read_mask: STENCIL_READ_MASK,
            // Nothing writes the plane, so the second draw is tested against the
            // same value the first was.
            write_mask: 0,
        }),
        bias: crcbl::hal::DepthBias::default(),
    };

    let outcome = match raster.pipeline(
        device,
        "stencil reference",
        PrimitiveState::default(),
        Some(depth_stencil),
    ) {
        Err(error) => Exercise::Refused(error),
        Ok(pipeline) => {
            let read = raster.render(headless, "stencil reference", Some(&target), |encoder| {
                // Before the bind, not after it: see the doc comment.
                encoder.set_stencil_reference(STENCIL_CLEARED);
                encoder.bind_graphics_pipeline(pipeline);
                encoder.bind_group(0, raster.pair_group, &[], raster.pipeline_layout);
                encoder.draw(0..RASTER_TRIANGLE_VERTICES, 0..1);
                encoder.set_stencil_reference(STENCIL_MISS);
                encoder.draw(0..RASTER_TRIANGLE_VERTICES * 2, 0..1);
            });
            device.destroy_graphics_pipeline(pipeline);
            match read {
                Err(error) => Exercise::Refused(error),
                Ok(read) if read.is(RASTER_FIRST) => Exercise::Worked,
                Ok(read) if read.is(RASTER_SECOND) => {
                    eprintln!(
                        "crcbl hal seam e2e: the draw at {STENCIL_MISS:#04x} was not discarded, so \
                         the second set_stencil_reference changed nothing — {}",
                        read.diagnosis()
                    );
                    Exercise::SilentlyIgnored
                }
                Ok(read) if read.is(RASTER_BACKGROUND) => {
                    eprintln!(
                        "crcbl hal seam e2e: neither draw survived, so the \
                         set_stencil_reference at {STENCIL_CLEARED:#04x} was not in force at the \
                         first draw — it was dropped, or binding the pipeline replaced it — {}",
                        read.diagnosis()
                    );
                    Exercise::SilentlyIgnored
                }
                Ok(read) => panic!(
                    "the stencil pass left a colour this exercise never draws: {}",
                    read.diagnosis()
                ),
            }
        }
    };

    raster.destroy(device);
    target.destroy(device);
    outcome
}

/// Whether this device can record more than one draw from a single indirect
/// call, which both indirect exercises need to observe anything at all.
fn can_multi_draw(headless: &Headless) -> bool {
    let caps = headless.device.caps();
    caps.features.contains(Features::MULTI_DRAW_INDIRECT)
        && caps.limits.max_draw_indirect_count >= RASTER_INDIRECT_DRAWS
}

/// Drives [`Capability::DrawIndirectCount`] and reports whether the draw count
/// came out of the buffer that holds it.
///
/// # What is asserted, and why a backend that does nothing cannot pass it
///
/// Two argument structures sit at `args_offset`: the first draws one triangle,
/// the second draws both. The count buffer holds a `1`, and `max_draw_count` is
/// [`RASTER_INDIRECT_DRAWS`] — so the whole claim is that the smaller number
/// wins. [`RASTER_FIRST`] in the centre is the only reading that follows;
/// [`RASTER_SECOND`] means the second structure was executed anyway, which is a
/// count taken from `max_draw_count` rather than from memory and is the failure
/// this capability exists to name.
///
/// Both offsets carry a decoy at the value a backend that dropped them would
/// read — an argument structure at zero that draws both triangles, and a count
/// of [`RASTER_INDIRECT_DRAWS`] at zero — so "the offsets were honoured" is
/// checkable rather than assumed. Both decoys produce [`RASTER_SECOND`], which
/// is the arm that panics.
fn exercise_draw_indirect_count(headless: &Headless) -> Exercise {
    /// Where the arguments the call actually names begin. Non-zero, so the
    /// decoy at zero is what an ignored offset reaches.
    const ARGS_AT: u64 = 32;
    /// Where the count the call actually names sits, for the same reason.
    const COUNT_AT: u64 = 4;

    if !can_multi_draw(headless) {
        return Exercise::Unexercised(NO_MULTI_DRAW_INDIRECT);
    }
    let device = headless.device.as_ref();
    let raster = Raster::open(headless);

    let mut args = vec![0u8; ARGS_AT as usize + 2 * DRAW_ARGS_TIGHT_STRIDE as usize];
    args[..DRAW_ARGS_TIGHT_STRIDE as usize]
        .copy_from_slice(&raster_draw_args(RASTER_TRIANGLE_VERTICES * 2));
    args[ARGS_AT as usize..ARGS_AT as usize + DRAW_ARGS_TIGHT_STRIDE as usize]
        .copy_from_slice(&raster_draw_args(RASTER_TRIANGLE_VERTICES));
    args[ARGS_AT as usize + DRAW_ARGS_TIGHT_STRIDE as usize..]
        .copy_from_slice(&raster_draw_args(RASTER_TRIANGLE_VERTICES * 2));
    let mut counts = Vec::new();
    counts.extend(RASTER_INDIRECT_DRAWS.to_le_bytes());
    counts.extend(1u32.to_le_bytes());

    let args = raster_indirect_buffer(headless, "raster indirect count args", &args);
    let counts = raster_indirect_buffer(headless, "raster indirect count", &counts);

    let outcome = match raster.pipeline(
        device,
        "draw indirect count",
        PrimitiveState::default(),
        None,
    ) {
        Err(error) => Exercise::Refused(error),
        Ok(pipeline) => {
            let read = raster.render(headless, "draw indirect count", None, |encoder| {
                encoder.bind_graphics_pipeline(pipeline);
                encoder.bind_group(0, raster.pair_group, &[], raster.pipeline_layout);
                encoder.draw_indirect_count(&crcbl::hal::DrawIndirectCount {
                    args,
                    args_offset: ARGS_AT,
                    count_buffer: counts,
                    count_offset: COUNT_AT,
                    max_draw_count: RASTER_INDIRECT_DRAWS,
                    stride: DRAW_ARGS_TIGHT_STRIDE,
                });
            });
            device.destroy_graphics_pipeline(pipeline);
            match read {
                Err(error) => Exercise::Refused(error),
                Ok(read) if read.is(RASTER_FIRST) => Exercise::Worked,
                Ok(read) if read.is(RASTER_BACKGROUND) => {
                    eprintln!(
                        "crcbl hal seam e2e: no draw at all reached the target — {}",
                        read.diagnosis()
                    );
                    Exercise::SilentlyIgnored
                }
                Ok(read) if read.is(RASTER_SECOND) => panic!(
                    "both argument structures were executed against a count buffer holding one, \
                     so the draw count came from max_draw_count ({RASTER_INDIRECT_DRAWS}) or from \
                     the decoy at offset zero rather than from {COUNT_AT} of the count buffer. A \
                     pass whose size the CPU never learns cannot be built on a count that is \
                     ignored.\n  {}",
                    read.diagnosis()
                ),
                Ok(read) => panic!(
                    "the indirect pass left a colour this exercise never draws: {}",
                    read.diagnosis()
                ),
            }
        }
    };

    device.destroy_buffer(counts);
    device.destroy_buffer(args);
    raster.destroy(device);
    outcome
}

/// Drives [`Capability::IndirectArgumentPaddedStride`] and reports whether the
/// second argument structure was read where the stride put it.
///
/// # What is asserted, and why a backend that does nothing cannot pass it
///
/// Three argument structures, and the draw asks for two of them at
/// [`DRAW_ARGS_PADDED_STRIDE`]. The first draws nothing at all, so the whole
/// picture is decided by *where the second one was read from*: at the padded
/// stride it is a structure drawing one triangle, and at the tight stride it is
/// a decoy drawing both.
///
/// * [`RASTER_FIRST`] — the stride was honoured. [`Exercise::Worked`].
/// * [`RASTER_SECOND`] — the arguments were walked at
///   [`DRAW_ARGS_TIGHT_STRIDE`] and the decoy between them was executed, which
///   is reading the wrong words silently. A panic, because the call did
///   something and did the wrong thing. The one backend that used to refuse
///   this rather than do it was `crcbl-wgpu`, deleted in `6b5e17a`; every
///   surviving backend answers `Yes` here, `crcbl-webgpu` included, where the
///   replayer walks the stride itself.
/// * [`RASTER_BACKGROUND`] — nothing was drawn, so the call was accepted and
///   dropped. [`Exercise::SilentlyIgnored`].
///
/// This is the first positive call to a padded stride anywhere in the workspace:
/// every other indirect call site — `crcbl-vk`'s `vk_e2e/indirect.rs`,
/// `tests/draw_gen_e2e` and the render graph itself — passes the tight one, so
/// "draws indirectly" was proven and "honours a stride" was not.
fn exercise_indirect_argument_padded_stride(headless: &Headless) -> Exercise {
    if !can_multi_draw(headless) {
        return Exercise::Unexercised(NO_MULTI_DRAW_INDIRECT);
    }
    let device = headless.device.as_ref();
    let raster = Raster::open(headless);

    let mut args = vec![0u8; DRAW_ARGS_PADDED_STRIDE as usize + DRAW_ARGS_TIGHT_STRIDE as usize];
    let put = |args: &mut Vec<u8>, at: usize, source: [u8; DRAW_ARGS_TIGHT_STRIDE as usize]| {
        args[at..at + source.len()].copy_from_slice(&source);
    };
    // The first argument draws nothing, so neither reading of the stride is
    // decided by it.
    put(&mut args, 0, raster_draw_args(0));
    // The decoy a tightly packed walk lands on.
    put(
        &mut args,
        DRAW_ARGS_TIGHT_STRIDE as usize,
        raster_draw_args(RASTER_TRIANGLE_VERTICES * 2),
    );
    // The second argument, where the stride says it is.
    put(
        &mut args,
        DRAW_ARGS_PADDED_STRIDE as usize,
        raster_draw_args(RASTER_TRIANGLE_VERTICES),
    );
    let args = raster_indirect_buffer(headless, "raster padded stride args", &args);

    let outcome = match raster.pipeline(device, "padded stride", PrimitiveState::default(), None) {
        Err(error) => Exercise::Refused(error),
        Ok(pipeline) => {
            let read = raster.render(headless, "padded stride", None, |encoder| {
                encoder.bind_graphics_pipeline(pipeline);
                encoder.bind_group(0, raster.pair_group, &[], raster.pipeline_layout);
                encoder.draw_indirect(&DrawIndirect {
                    args,
                    offset: 0,
                    draw_count: RASTER_INDIRECT_DRAWS,
                    stride: DRAW_ARGS_PADDED_STRIDE,
                });
            });
            device.destroy_graphics_pipeline(pipeline);
            match read {
                Err(error) => Exercise::Refused(error),
                Ok(read) if read.is(RASTER_FIRST) => Exercise::Worked,
                Ok(read) if read.is(RASTER_BACKGROUND) => {
                    eprintln!(
                        "crcbl hal seam e2e: no draw at all reached the target — {}",
                        read.diagnosis()
                    );
                    Exercise::SilentlyIgnored
                }
                Ok(read) if read.is(RASTER_SECOND) => panic!(
                    "the decoy between the two arguments was executed, so a stride of \
                     {DRAW_ARGS_PADDED_STRIDE} was accepted and the arguments were walked at \
                     {DRAW_ARGS_TIGHT_STRIDE}. Every word after the first structure is then read \
                     from the wrong place, which is a wrong picture with a clean log.\n  {}",
                    read.diagnosis()
                ),
                Ok(read) => panic!(
                    "the indirect pass left a colour this exercise never draws: {}",
                    read.diagnosis()
                ),
            }
        }
    };

    device.destroy_buffer(args);
    raster.destroy(device);
    outcome
}

/// The word every element of the push-constant destination holds before the
/// dispatch.
///
/// [`FILL_POISON`]'s job for this exercise, and a different value from it and
/// from [`IMAGE_COPY_POISON`] so that "this destination was never written" stays
/// separable from "some other exercise's primer reached it". A destination that
/// still holds this after the dispatch is a dispatch that produced nothing at
/// all — which is a different reading from the constants having gone missing,
/// and [`push_constant_dispatch`] keeps the two apart.
const PUSH_CONSTANT_POISON: u32 = 0x5A5A_A5A5;

/// The block [`exercise_push_constants_on_compute`] pushes.
///
/// **One distinct word per index, not a repeated value.**
/// `push_constant_probe.slang` writes `values[i]` into element `i` of its
/// destination and reads nothing else, so a block that arrived shifted — a range
/// whose offset a backend applied twice, a root-constant slot off by one —
/// comes back as the right four words in the wrong four elements. A repeated
/// value could not tell that from a correct dispatch; this pattern makes the
/// comparison name the element that moved.
///
/// Each word differs from every other in both halves, so a rotation and a
/// byte-swap are both visible rather than folded into each other.
const PUSHED: crcbl::shaders::push_constant_probe::Constants =
    crcbl::shaders::push_constant_probe::Constants {
        values: [0x1111_0001, 0x2222_0002, 0x3333_0003, 0x4444_0004],
    };

// The premise both `SilentlyIgnored` arms rest on, checked at compile time
// because every side is a constant. A pushed word equal to the poison would make
// a dispatch that never ran read back as one that worked, and a pushed word of
// zero would do the same for a block that arrived empty — which are the two
// confusions those arms exist to separate.
const _: () = {
    let mut index = 0;
    while index < crcbl::shaders::push_constant_probe::WORD_COUNT {
        assert!(
            PUSHED.values[index] != PUSH_CONSTANT_POISON,
            "a word of PUSHED equals PUSH_CONSTANT_POISON, so a dispatch that never ran and one \
             that delivered the block are no longer distinguishable"
        );
        assert!(
            PUSHED.values[index] != 0,
            "a word of PUSHED is zero, so a push-constant block that arrived empty and one that \
             was delivered are no longer distinguishable"
        );
        index += 1;
    }
};

/// Drives [`Capability::PushConstants`] through **both** halves of the surface
/// it names, and reports the first one that is not [`Exercise::Worked`].
///
/// The capability is stage-agnostic by its own definition — `push_constants` and
/// the `PipelineLayoutDesc::push_constants` range they are written through — and
/// for a long time only [`exercise_push_constants_on_compute`] existed, so it
/// counted as driven on every backend while nothing anywhere pushed a constant
/// through a graphics pipeline. The plumbing is genuinely different there: D3D12
/// computes the root parameter's shader visibility from the range's stages,
/// Metal has one argument table per stage and sends the block with
/// `setVertexBytes:`/`setFragmentBytes:` — recorded rather than written eagerly —
/// and Vulkan's stage mask is a value a compute-only range never varies.
/// [`exercise_push_constants_on_graphics`] is the other half.
///
/// **Sequential rather than combined**, because the two halves are not
/// independent answers about one device: the graphics half builds its own
/// pipeline layout with its own range, and a backend that refuses at layout
/// creation refuses both. Running it after a compute half that did not work
/// would report the same refusal twice and hide which one was asked first.
fn exercise_push_constants(headless: &Headless) -> Exercise {
    match exercise_push_constants_on_compute(headless) {
        Exercise::Worked => exercise_push_constants_on_graphics(headless),
        other => other,
    }
}

/// Drives [`Capability::PushConstants`] from a compute dispatch and reports
/// whether the words the caller pushed are the words the shader read.
///
/// # Why this one needs its own fixture
///
/// [`ComputeProbe`] cannot answer it. Its layout declares
/// `push_constants: None` and `compute_probe.slang` reads its element count from
/// a bound uniform, so nothing in that dispatch could distinguish a push constant
/// that arrived from one that never left. This builds the second compute
/// fixture in the file instead: one storage buffer, no uniform, no source — and
/// `crcbl_shaders::PUSH_CONSTANT_PROBE`, whose only input besides the destination
/// binding is the block.
///
/// # What is asserted, and why a backend that does nothing cannot pass it
///
/// The destination is primed with [`PUSH_CONSTANT_POISON`] through a copy from a
/// host buffer — a copy rather than a `fill_buffer`, for the reason
/// [`ComputeProbe::run`] gives: wgpu offers only a zero fill, and zero is exactly
/// the value an unwritten destination is indistinguishable from. Then
/// [`PUSHED`] goes in through
/// [`push_constants`](crcbl::hal::CommandEncoder::push_constants), one group is
/// dispatched, and the buffer is read back and compared word for word:
///
/// * the pattern — the shader obtained four values it had no other way to reach,
///   at the elements the block's offsets put them. [`Exercise::Worked`].
/// * [`PUSH_CONSTANT_POISON`] throughout — the prime landed and the dispatch
///   produced nothing at all. [`Exercise::SilentlyIgnored`].
/// * zeros throughout — the prime was overwritten, so the shader ran, and the
///   block it read was empty. This is what a `push_constants` call that was
///   accepted and dropped actually looks like, and it is deliberately a
///   *different* value from the poison so the two stay apart:
///   [`Exercise::SilentlyIgnored`] as well, because both are the seam accepting
///   a request and changing nothing observable.
/// * anything else — the constants reached the shader wrong, and the panic names
///   the first element that disagreed.
///
/// # A refusal can arrive at three different points
///
/// [`Capability::PushConstants`] is the one capability whose refusal the seam
/// pins to a *place*: `crcbl_hal::pipeline` requires
/// [`PipelineLayoutDesc::push_constants`](crcbl::hal::PipelineLayoutDesc) to fail
/// at layout creation rather than have the writes dropped later. Three backends
/// do exactly that today — `crcbl-mtl`, `crcbl-dx12` and `crcbl-wgpu` all return
/// [`HalError::Unsupported`] from `create_pipeline_layout`. But a backend that
/// gains the range and not the write would refuse at pipeline creation, and one
/// that defers its errors would refuse at
/// [`finish`](crcbl::hal::CommandEncoder::finish), so all three points return
/// [`Exercise::Refused`] rather than the first one alone. Anything less and the
/// two open `PushConstants` divergence rows would have to be re-plumbed the day
/// they are closed.
///
/// The shader module is the one step that panics instead. It is reached only
/// after the layout was accepted, which no backend without push constants gets
/// to — and the artifact ships SPIR-V, MSL and DXIL but deliberately no WGSL,
/// because WGSL has no push constants at all. A backend that accepted the range
/// and then rejected the artifact is a fixture that needs looking at, not a
/// capability answer.
///
/// # Why the shader selects components with a switch rather than indexing
///
/// **Fixed, and recorded because the measurement is the reason the shader is
/// written the way it is.** It was once a real failure of the fixture rather
/// than of any backend, and it made a `CRCBL_GPU=vk` run go red on an AMD card
/// while passing on a software one. `push_constant_probe.slang` used to read
/// `constants.values[index]` with `index` taken from `SV_DispatchThreadID`,
/// which Slang emits as an `OpAccessChain` into the push-constant vector with a
/// per-invocation index.
/// NIR defines the offset of a `load_push_constant` as dynamically uniform, so
/// ACO is entitled to scalarise it — and does. Measured here on Mesa 26.1.7:
/// `RADV_DEBUG=shaders` shows `v_readfirstlane_b32 s3, v0` ahead of the
/// `s_load_b32`, so every invocation reads the block at lane 0's offset and all
/// four elements come back holding `values[0]`. Both RADV adapters on this
/// machine — a discrete Navi31 and an integrated Raphael — answer that way;
/// llvmpipe, which addresses per lane, returns the four words correctly.
///
/// Nothing this file could do fixed it: the exercise, the range and the write were
/// all correct, and the shader was what had to change. It now selects constant
/// components (`values.x`/`.y`/`.z`/`.w` through a `switch`) rather than
/// indexing through a pointer, which keeps the one-`uint4` layout the source
/// argues for and takes the divergent index out of the load. The exercise
/// passes on both RADV adapters here as well as on llvmpipe.
///
/// **Do not put the index back.** It reads more naturally and it is the form
/// that was measured wrong.
fn exercise_push_constants_on_compute(headless: &Headless) -> Exercise {
    use crcbl::shaders::push_constant_probe::{CONSTANTS_SIZE, WORD_COUNT, WORKGROUP_SIZE};

    let device = headless.device.as_ref();
    let bytes = u64::from(CONSTANTS_SIZE);

    let prime = device
        .create_buffer(&BufferDesc {
            label: Some("push constant prime"),
            size: bytes,
            usage: BufferUsage::TRANSFER_SRC,
            memory: MemoryLocation::HostUpload,
        })
        .expect("a host-upload buffer");
    let destination = device
        .create_buffer(&BufferDesc {
            label: Some("push constant destination"),
            size: bytes,
            usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_DST | BufferUsage::TRANSFER_SRC,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("a device-local buffer");
    let staging = device
        .create_buffer(&BufferDesc {
            label: Some("push constant readback"),
            size: bytes,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a host-readback buffer");

    let primer: Vec<u8> = core::iter::repeat_n(PUSH_CONSTANT_POISON, WORD_COUNT)
        .flat_map(u32::to_le_bytes)
        .collect();
    device
        .write_buffer(prime, 0, &primer)
        .expect("a host-upload buffer is what write_buffer is for");

    // One binding, because the shader has one: the block is the only other
    // thing the dispatch reads, which is what makes the readback evidence about
    // the block rather than about whatever else was bound.
    let layout_entries = [crcbl::hal::BindGroupLayoutEntry {
        binding: 0,
        visibility: crcbl::hal::ShaderStages::COMPUTE,
        kind: crcbl::hal::BindingKind::StorageBuffer {
            read_only: false,
            dynamic: false,
        },
        count: 1,
        flags: crcbl::hal::BindingFlags::empty(),
    }];
    let bind_group_layout = device
        .create_bind_group_layout(&crcbl::hal::BindGroupLayoutDesc {
            label: Some("push constant probe"),
            entries: &layout_entries,
        })
        .expect("a one-binding layout");
    let group_entries = [crcbl::hal::BindGroupEntry {
        binding: 0,
        array_index: 0,
        resource: crcbl::hal::BindingResource::whole_buffer(destination),
    }];
    let bind_group = device
        .create_bind_group(&crcbl::hal::BindGroupDesc {
            label: Some("push constant probe"),
            layout: bind_group_layout,
            entries: &group_entries,
            variable_count: None,
        })
        .expect("a bind group");

    let set_layouts = [bind_group_layout];
    // The range the artifacts were measured at: the SPIR-V decorates the block's
    // only member `Offset 0` and nothing sits in front of it on any target, so
    // the whole block is one range starting at zero.
    let outcome = match device.create_pipeline_layout(&crcbl::hal::PipelineLayoutDesc {
        label: Some("push constant probe"),
        bind_group_layouts: &set_layouts,
        push_constants: Some(crcbl::hal::PushConstantRange {
            stages: crcbl::hal::ShaderStages::COMPUTE,
            offset: 0,
            size: CONSTANTS_SIZE,
        }),
    }) {
        Err(error) => Exercise::Refused(error),
        Ok(pipeline_layout) => {
            let module = device
                .create_shader_module(&crcbl::hal::ShaderModuleDesc {
                    label: Some("push_constant_probe.slang"),
                    spirv: crcbl::shaders::PUSH_CONSTANT_PROBE.spirv(),
                    // `None`, and that is the artifact rather than an omission:
                    // the source declares no `wgsl` target because WGSL has no
                    // push constants.
                    wgsl: crcbl::shaders::PUSH_CONSTANT_PROBE.wgsl(),
                    msl: crcbl::shaders::PUSH_CONSTANT_PROBE.msl(),
                    dxil: &crcbl::shaders::PUSH_CONSTANT_PROBE.dxil_containers(),
                })
                .expect(
                    "this backend accepted a push-constant range and then refused the committed \
                     push_constant_probe artifact — see the note above; that is a fixture problem \
                     rather than a capability answer",
                );
            let entry_point = crcbl::shaders::PUSH_CONSTANT_PROBE
                .entry_point(crcbl::shaders::Stage::Compute)
                .expect("the probe has exactly one compute entry point");
            let outcome = match device.create_compute_pipeline(&crcbl::hal::ComputePipelineDesc {
                label: Some("push constant probe"),
                layout: pipeline_layout,
                compute: crcbl::hal::ShaderEntry {
                    module,
                    entry_point,
                },
                workgroup_size: [WORKGROUP_SIZE, 1, 1],
            }) {
                Err(error) => Exercise::Refused(error),
                Ok(pipeline) => {
                    let outcome =
                        push_constant_dispatch(headless, prime, destination, staging, bytes, |e| {
                            e.bind_compute_pipeline(pipeline);
                            e.bind_group(0, bind_group, &[], pipeline_layout);
                            e.push_constants(
                                crcbl::hal::ShaderStages::COMPUTE,
                                0,
                                &PUSHED.to_bytes(),
                                pipeline_layout,
                            );
                            // One group, which is wider than WORD_COUNT on
                            // purpose: the shader's bounds check is a branch this
                            // dispatch really takes.
                            e.dispatch(1, 1, 1);
                        });
                    device.destroy_compute_pipeline(pipeline);
                    outcome
                }
            };
            device.destroy_shader_module(module);
            device.destroy_pipeline_layout(pipeline_layout);
            outcome
        }
    };

    device.destroy_bind_group(bind_group);
    device.destroy_bind_group_layout(bind_group_layout);
    device.destroy_buffer(staging);
    device.destroy_buffer(destination);
    device.destroy_buffer(prime);
    outcome
}

/// Primes the destination, runs `record` inside a compute pass, and reads the
/// destination back as words — or hands back the error `finish` refused with.
///
/// Split out of [`exercise_push_constants_on_compute`] so the recording — which
/// is the only part that is about push constants — sits next to the pipeline it
/// needs, with the barriers and the submit out of the way, and shared with
/// [`exercise_bindless_descriptor_array`] once that arrived: the two differ in
/// what they record and in what the words are compared against, and in nothing
/// between. The barrier shape is [`ComputeProbe::run`]'s, and for its reasons.
///
/// # Errors
///
/// Whatever [`finish`](crcbl::hal::CommandEncoder::finish) refused with, for a
/// backend that defers its errors to the end of recording. Every caller turns
/// that into [`Exercise::Refused`].
fn primed_dispatch(
    headless: &Headless,
    prime: crcbl::hal::BufferHandle,
    destination: crcbl::hal::BufferHandle,
    staging: crcbl::hal::BufferHandle,
    bytes: u64,
    record: impl FnOnce(&mut dyn crcbl::hal::CommandEncoder),
) -> Result<Vec<u32>, HalError> {
    let device = headless.device.as_ref();
    let barrier = |buffer, from, to| crcbl::hal::BufferBarrier {
        buffer,
        from,
        to,
        queue_transfer: None,
    };
    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("push constant exercise"),
        queue: headless.queue,
    });
    encoder.pipeline_barrier(&Barriers {
        buffers: &[barrier(
            destination,
            ResourceState::Undefined,
            ResourceState::TransferDst,
        )],
        ..Barriers::default()
    });
    encoder.copy_buffer_to_buffer(&crcbl::hal::BufferCopy {
        src: prime,
        src_offset: 0,
        dst: destination,
        dst_offset: 0,
        size: bytes,
    });
    encoder.pipeline_barrier(&Barriers {
        buffers: &[barrier(
            destination,
            ResourceState::TransferDst,
            ResourceState::ShaderReadWrite,
        )],
        ..Barriers::default()
    });

    encoder.begin_compute_pass(&crcbl::hal::ComputePassDesc {
        label: Some("push constant probe"),
        timestamp_writes: None,
    });
    record(encoder.as_mut());
    encoder.end_compute_pass();

    encoder.pipeline_barrier(&Barriers {
        buffers: &[barrier(
            destination,
            ResourceState::ShaderReadWrite,
            ResourceState::TransferSrc,
        )],
        ..Barriers::default()
    });
    encoder.copy_buffer_to_buffer(&crcbl::hal::BufferCopy {
        src: destination,
        src_offset: 0,
        dst: staging,
        dst_offset: 0,
        size: bytes,
    });

    let commands = encoder.finish()?;
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");
    device.wait_idle().expect("idle");
    device.destroy_command_buffer(commands);

    let mut read = poisoned(bytes as usize);
    headless.readback(staging, bytes, &mut read);
    Ok(read
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
        .collect())
}

/// Says which of the three outcomes a push-constant dispatch produced.
///
/// The classification is what is left of [`primed_dispatch`]'s old body once the
/// mechanics moved out: the words come from a dispatch of
/// `push_constant_probe.slang`, and every arm below is about [`PUSHED`].
fn push_constant_dispatch(
    headless: &Headless,
    prime: crcbl::hal::BufferHandle,
    destination: crcbl::hal::BufferHandle,
    staging: crcbl::hal::BufferHandle,
    bytes: u64,
    record: impl FnOnce(&mut dyn crcbl::hal::CommandEncoder),
) -> Exercise {
    let words = match primed_dispatch(headless, prime, destination, staging, bytes, record) {
        Err(error) => return Exercise::Refused(error),
        Ok(words) => words,
    };

    if words == PUSHED.values {
        Exercise::Worked
    } else if words.iter().all(|word| *word == PUSH_CONSTANT_POISON) {
        // The prime is all that ever reached the buffer, so the dispatch itself
        // produced nothing.
        eprintln!(
            "crcbl hal seam e2e: the destination still holds {PUSH_CONSTANT_POISON:#010x} \
             throughout, so the dispatch wrote nothing at all"
        );
        Exercise::SilentlyIgnored
    } else if words.iter().all(|word| *word == 0) {
        // **The shape a dropped `push_constants` actually takes**, and it is not
        // the poison: the dispatch ran — the poison is gone, so the shader did
        // write every element — and the block it read was empty. Measured rather
        // than assumed, by deleting the `push_constants` call from the recording
        // above and running it: the readback came back four zeros where the
        // poison had been. A backend that accepts the write and never delivers
        // it lands here, which is the outcome the seam's rule against a quiet
        // no-op exists to catch.
        eprintln!(
            "crcbl hal seam e2e: the dispatch overwrote the prime with zeros, so the shader ran \
             and the push-constant block it read was empty"
        );
        Exercise::SilentlyIgnored
    } else {
        let index = words
            .iter()
            .zip(PUSHED.values)
            .position(|(got, want)| *got != want)
            .expect("the two agree word for word only on the arm above");
        panic!(
            "element {index} of the destination came back {:#010x} where {:#010x} was pushed. The \
             dispatch left {words:#010x?} and the block pushed was {:#010x?}. The shader writes \
             values[i] into element i and reads nothing else, so every word here came out of the \
             block and landed in the wrong place — the right constants written somewhere else, \
             which is a wrong picture with a clean log. Three things produce it, and the readback \
             above says which: the range reached the shader at the wrong offset, only part of the \
             block was delivered, or every element holds one and the same pushed word. That \
             last shape had a known cause once — a driver scalarising a divergent index into the \
             block — and the shader was changed to a component `switch` so it cannot arise that \
             way any more; see `exercise_push_constants_on_compute`. Seeing it now is a new \
             defect rather than that one.",
            words[index], PUSHED.values[index], PUSHED.values,
        )
    }
}

/// The clip-space rectangle the **first** draw's block spans.
///
/// The left band of the colour target, full height. Full height is not decoration:
/// the seam samples the target's centre row, and a rectangle symmetric about
/// `y = 0` is one whose coverage of that row does not depend on which sign
/// convention clip-space Y carries — [`RASTER_TRIANGLE`]'s reason for being
/// symmetric about `x = 0`, and the same reason this exercise separates its two
/// draws by *column* and never by row.
const PUSH_CONSTANT_FIRST_RECT: [f32; 4] = [-0.9, -1.0, -0.1, 1.0];

/// The clip-space rectangle the **second** draw's block spans: the right band,
/// disjoint from [`PUSH_CONSTANT_FIRST_RECT`] with the target's middle columns
/// left as background between them.
const PUSH_CONSTANT_SECOND_RECT: [f32; 4] = [0.1, -1.0, 0.9, 1.0];

/// The column [`exercise_push_constants_on_graphics`] reads the first draw's
/// verdict from, well inside [`PUSH_CONSTANT_FIRST_RECT`] and well outside the
/// second's — which the exercise checks rather than assumes.
const PUSH_CONSTANT_FIRST_COLUMN: u32 = RASTER_WIDTH / 4;

/// The column the second draw's verdict is read from.
const PUSH_CONSTANT_SECOND_COLUMN: u32 = RASTER_WIDTH - RASTER_WIDTH / 4;

/// Whether the centre of `column` falls strictly inside the clip-space span
/// `rect` covers.
///
/// The premise both verdicts rest on, so it is computed from the rectangle the
/// shader is actually handed rather than restated as a pixel range beside it: a
/// rectangle edited without its sample column moving with it would otherwise
/// read the background and be scored as a block that never arrived.
fn push_constant_column_is_covered(rect: [f32; 4], column: u32) -> bool {
    let [min_x, _, max_x, _] = rect;
    let centre = (f64::from(column) + 0.5) / f64::from(RASTER_WIDTH) * 2.0 - 1.0;
    centre > f64::from(min_x) && centre < f64::from(max_x)
}

/// What the two draws left in the colour target.
#[derive(Debug)]
struct BandRead {
    /// The texel at [`PUSH_CONSTANT_FIRST_COLUMN`] of the centre row.
    first: [u8; 4],
    /// The texel at [`PUSH_CONSTANT_SECOND_COLUMN`] of the centre row.
    second: [u8; 4],
    /// Texels that are not [`RASTER_BACKGROUND`], for a panic that has to say
    /// what the pass actually covered.
    drawn: usize,
}

impl BandRead {
    fn from_bytes(read: &[u8]) -> Self {
        let row = RASTER_HEIGHT / 2;
        Self {
            first: raster_texel(read, PUSH_CONSTANT_FIRST_COLUMN, row),
            second: raster_texel(read, PUSH_CONSTANT_SECOND_COLUMN, row),
            drawn: raster_drawn(read),
        }
    }

    /// The two texels and the levels each colour in play would have put there.
    fn diagnosis(&self) -> String {
        format!(
            "column {PUSH_CONSTANT_FIRST_COLUMN} is {:?} and column {PUSH_CONSTANT_SECOND_COLUMN} \
             is {:?}, with {} of {} texels not the background.\n  background {:?}\n  first {:?}\n  \
             second {:?}",
            self.first,
            self.second,
            self.drawn,
            RASTER_WIDTH * RASTER_HEIGHT,
            raster_levels(RASTER_BACKGROUND),
            raster_levels(RASTER_FIRST),
            raster_levels(RASTER_SECOND),
        )
    }
}

/// Drives [`Capability::PushConstants`] from a graphics pipeline: **two draws in
/// one render pass, with different blocks, each asserting it saw its own.**
///
/// # Two draws, because one cannot tell a copied block from a shared one
///
/// A single draw is satisfied by any implementation that delivers *a* block —
/// including one that keeps a pointer into a shadow a later
/// [`push_constants`](crcbl::hal::CommandEncoder::push_constants) splices in
/// place, which is exactly what `crcbl-mtl`'s `RenderCommand::PushConstants`
/// carries a `Vec<u8>` copy to prevent. So this pushes one block, draws, pushes
/// a second, and draws again, and the two draws are told apart in the picture
/// rather than by which call was made.
///
/// # The two stages read different halves of the block
///
/// `push_constant_raster.slang` takes its **rectangle** from the block in the
/// vertex stage and its **colour** from the block in the fragment stage, and
/// pulls no vertices from anywhere — the pipeline layout carries no bind group
/// at all. So a pixel of the expected colour in the expected column is a value
/// the pipeline had no source for other than the block, and a block that reached
/// one stage and not the other is visible rather than folded into one verdict:
///
/// * the fragment stage alone — `rect` arrived zero, all four corners collapse
///   onto the clip-space origin and **nothing is drawn at all**;
/// * the vertex stage alone — the bands are drawn in black, which no colour here
///   is, and the final arm names the texel.
///
/// # What each reading means
///
/// The first draw's block spans [`PUSH_CONSTANT_FIRST_RECT`] in [`RASTER_FIRST`]
/// and the second's spans [`PUSH_CONSTANT_SECOND_RECT`] in [`RASTER_SECOND`],
/// two disjoint columns of the target:
///
/// * each column its own colour — every draw saw its own block, in both stages.
///   [`Exercise::Worked`].
/// * both columns the background — neither rectangle arrived, so the block was
///   accepted and never delivered. [`Exercise::SilentlyIgnored`], which is what
///   the seam's rule against a quiet no-op is for.
/// * the first column drawn and the second background — **the second draw saw
///   the first draw's block**, and this is the failure the two-draw shape exists
///   to catch.
/// * the first column background and the second drawn — the first draw saw the
///   *second's* block, which is what a write applied to the whole pass rather
///   than to the draws after it looks like.
/// * anything else — the panic prints both texels and every colour in play.
///
/// # A refusal can arrive at three different points
///
/// [`exercise_push_constants_on_compute`]'s note applies unchanged, with one
/// addition: this layout's range names `ShaderStages::VERTEX | FRAGMENT` where
/// that one names `ShaderStages::COMPUTE`, so a backend that has push constants
/// for compute and not for the graphics stages refuses *here* and is reported
/// rather than hidden. That is the whole point of the second half.
fn exercise_push_constants_on_graphics(headless: &Headless) -> Exercise {
    use crcbl::shaders::push_constant_raster::{CONSTANTS_SIZE, Constants, VERTEX_COUNT};

    let device = headless.device.as_ref();

    // The premise both verdicts rest on: each draw's sample column is inside its
    // own rectangle and outside the other's, so "which block did this draw see"
    // is answerable from one texel per draw.
    for (which, rect, own, other) in [
        (
            "first",
            PUSH_CONSTANT_FIRST_RECT,
            PUSH_CONSTANT_FIRST_COLUMN,
            PUSH_CONSTANT_SECOND_COLUMN,
        ),
        (
            "second",
            PUSH_CONSTANT_SECOND_RECT,
            PUSH_CONSTANT_SECOND_COLUMN,
            PUSH_CONSTANT_FIRST_COLUMN,
        ),
    ] {
        assert!(
            push_constant_column_is_covered(rect, own),
            "the {which} draw's rectangle {rect:?} does not cover column {own}, which is where \
             this exercise reads its verdict"
        );
        assert!(
            !push_constant_column_is_covered(rect, other),
            "the {which} draw's rectangle {rect:?} covers column {other}, which is the other \
             draw's verdict — the two draws would no longer be separable"
        );
    }

    // Both graphics stages, because the shader reads the block from both — and
    // because a mask naming one is a mask a backend can get right by accident.
    let stages = crcbl::hal::ShaderStages::VERTEX | crcbl::hal::ShaderStages::FRAGMENT;

    // The fixture whole, for its colour target and its prime/clear/readback: the
    // geometry it stages and the layout it builds belong to `triangle.slang` and
    // go unused here, which costs two buffers and buys the same pass every other
    // raster exercise is read out of.
    let raster = Raster::open(headless);
    // No bind group: the shader pulls no vertices and reads nothing but the
    // block, which is what makes the picture evidence about the block.
    let outcome = match device.create_pipeline_layout(&crcbl::hal::PipelineLayoutDesc {
        label: Some("push constant raster"),
        bind_group_layouts: &[],
        push_constants: Some(crcbl::hal::PushConstantRange {
            stages,
            offset: 0,
            size: CONSTANTS_SIZE,
        }),
    }) {
        Err(error) => Exercise::Refused(error),
        Ok(pipeline_layout) => {
            let module = device
                .create_shader_module(&crcbl::hal::ShaderModuleDesc {
                    label: Some("push_constant_raster.slang"),
                    spirv: crcbl::shaders::PUSH_CONSTANT_RASTER.spirv(),
                    // `None`, and that is the artifact rather than an omission:
                    // the source declares no `wgsl` target because WGSL has no
                    // push constants.
                    wgsl: crcbl::shaders::PUSH_CONSTANT_RASTER.wgsl(),
                    msl: crcbl::shaders::PUSH_CONSTANT_RASTER.msl(),
                    dxil: &crcbl::shaders::PUSH_CONSTANT_RASTER.dxil_containers(),
                })
                .expect(
                    "this backend accepted a push-constant range over the graphics stages and then \
                     refused the committed push_constant_raster artifact — that is a fixture \
                     problem rather than a capability answer",
                );
            let outcome = match raster_pipeline(
                device,
                "push constant raster",
                pipeline_layout,
                module,
                &crcbl::shaders::PUSH_CONSTANT_RASTER,
                PrimitiveState::default(),
                None,
            ) {
                Err(error) => Exercise::Refused(error),
                Ok(pipeline) => {
                    let block = |color, rect| Constants { color, rect }.to_bytes();
                    let first = block(RASTER_FIRST, PUSH_CONSTANT_FIRST_RECT);
                    let second = block(RASTER_SECOND, PUSH_CONSTANT_SECOND_RECT);
                    let read =
                        raster.render_target(headless, "push constant raster", None, |encoder| {
                            encoder.bind_graphics_pipeline(pipeline);
                            encoder.push_constants(stages, 0, &first, pipeline_layout);
                            encoder.draw(0..VERTEX_COUNT, 0..1);
                            encoder.push_constants(stages, 0, &second, pipeline_layout);
                            encoder.draw(0..VERTEX_COUNT, 0..1);
                        });
                    device.destroy_graphics_pipeline(pipeline);
                    match read {
                        Err(error) => Exercise::Refused(error),
                        Ok(read) => push_constant_bands(&BandRead::from_bytes(&read)),
                    }
                }
            };
            device.destroy_shader_module(module);
            device.destroy_pipeline_layout(pipeline_layout);
            outcome
        }
    };

    raster.destroy(device);
    outcome
}

/// Which of [`exercise_push_constants_on_graphics`]' readings `read` is.
///
/// Split out so the arms and their messages sit together rather than four
/// levels inside the resource handling above.
fn push_constant_bands(read: &BandRead) -> Exercise {
    let background = raster_levels(RASTER_BACKGROUND);
    let first_drew = raster_texel_is(&read.first, raster_levels(RASTER_FIRST));
    let second_drew = raster_texel_is(&read.second, raster_levels(RASTER_SECOND));
    let first_clear = raster_texel_is(&read.first, background);
    let second_clear = raster_texel_is(&read.second, background);

    if first_drew && second_drew {
        Exercise::Worked
    } else if first_clear && second_clear {
        // The pass ran — `Raster::render_target` has already refused a target
        // that still holds the prime — and neither rectangle reached the vertex
        // stage, so both quads collapsed to no area. This is what a
        // `push_constants` call that was accepted and dropped looks like on a
        // graphics pipeline.
        eprintln!(
            "crcbl hal seam e2e: the pass cleared and neither draw covered anything, so no \
             rectangle reached the vertex stage and the push-constant block was never delivered"
        );
        Exercise::SilentlyIgnored
    } else if first_drew && second_clear {
        panic!(
            "the second draw drew the FIRST draw's rectangle: {}. Both draws saw the block pushed \
             before the first of them, so the second push_constants either never reached the \
             pipeline or was written into a buffer the first draw had already been given a pointer \
             to. That is the failure two draws exist to catch — one draw cannot tell a block that \
             was copied for it from one that is shared with the draw beside it.",
            read.diagnosis()
        )
    } else if first_clear && second_drew {
        panic!(
            "the first draw drew nothing and the second drew its own rectangle: {}. Both draws saw \
             the block pushed before the SECOND of them, so the write reached the whole pass \
             instead of only the draws recorded after it — a block applied eagerly at submission \
             rather than in the order the encoder recorded.",
            read.diagnosis()
        )
    } else {
        panic!(
            "the pass left colours this exercise never pushed: {}. The vertex stage takes its \
             rectangle and the fragment stage its colour from the same block and the pipeline \
             reads nothing else, so a band drawn in a colour that is neither of the two pushed is \
             a block that arrived corrupted or at the wrong offset — black bands are the shape a \
             block delivered to the vertex stage alone takes.",
            read.diagnosis()
        )
    }
}

/// What the destination the group starts out naming holds before either
/// dispatch.
///
/// Distinct from [`UPDATE_POISON_SECOND`] and not merely "some poison": the two
/// destinations are the same size and the same shape, so a prime that landed on
/// the wrong one is only visible if the primes differ. Both are also larger than
/// any word a dispatch can write — see the assertion below — which is what makes
/// "still primed" and "written" separable without comparing against the whole
/// expectation.
const UPDATE_POISON_FIRST: u32 = 0xC0FF_EE01;

/// What the destination the update repoints the group *to* holds before either
/// dispatch.
const UPDATE_POISON_SECOND: u32 = 0xF00D_BA02;

// The premises the four readings in `exercise_update_bind_group` rest on,
// checked at compile time because every side is a constant.
const _: () = {
    assert!(
        UPDATE_POISON_FIRST != UPDATE_POISON_SECOND,
        "the two destinations are primed with the same value, so a dispatch that wrote the wrong \
         one reads back exactly like one that wrote the right one"
    );
    assert!(
        UPDATE_POISON_FIRST != PROBE_SENTINEL && UPDATE_POISON_SECOND != PROBE_SENTINEL,
        "a poison equal to PROBE_SENTINEL would let another compute exercise's primer read as \
         this one's"
    );
    // `probe_source` is `index + 1` over `PROBE_ELEMENTS` elements and the
    // shader squares it, so this is the largest word either dispatch can write.
    let largest_written = PROBE_ELEMENTS * PROBE_ELEMENTS;
    assert!(
        UPDATE_POISON_FIRST > largest_written,
        "UPDATE_POISON_FIRST is inside the range of squares the probe writes, so an element the \
         dispatch did write is no longer distinguishable from one it did not"
    );
    assert!(
        UPDATE_POISON_SECOND > largest_written,
        "UPDATE_POISON_SECOND is inside the range of squares the probe writes, so an element the \
         dispatch did write is no longer distinguishable from one it did not"
    );
};

/// One of [`exercise_update_bind_group`]'s two destinations, with everything
/// that makes its contents evidence.
///
/// The three buffers travel together because they are one claim: `buffer` is
/// what the bind group may name, `prime` is the host copy of [`poison`](Self::poison)
/// that goes over it before every dispatch, and `staging` is where it comes home
/// through. Splitting them into loose arguments put the run helper over
/// `clippy::too_many_arguments` and made it possible to prime one destination
/// and read the other.
struct Destination {
    /// The device-local storage buffer the probe's binding 2 may point at.
    buffer: crcbl::hal::BufferHandle,
    /// A host buffer holding nothing but [`poison`](Self::poison), copied over
    /// [`buffer`](Self::buffer) before each dispatch. A copy rather than a
    /// `fill_buffer` for [`ComputeProbe::run`]'s reason: wgpu offers only a zero
    /// fill, and zero is exactly what an unwritten destination looks like.
    prime: crcbl::hal::BufferHandle,
    /// Host-readable copy target, so the result is asserted rather than assumed.
    staging: crcbl::hal::BufferHandle,
    /// This destination's own primer value.
    poison: u32,
}

impl Destination {
    fn open(headless: &Headless, label: &'static str, poison: u32) -> Self {
        let device = headless.device.as_ref();
        let buffer = device
            .create_buffer(&BufferDesc {
                label: Some(label),
                size: probe_bytes(),
                usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_DST | BufferUsage::TRANSFER_SRC,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("a device-local storage buffer");
        let prime = device
            .create_buffer(&BufferDesc {
                label: Some(label),
                size: probe_bytes(),
                usage: BufferUsage::TRANSFER_SRC,
                memory: MemoryLocation::HostUpload,
            })
            .expect("a host-upload buffer");
        let staging = device
            .create_buffer(&BufferDesc {
                label: Some(label),
                size: probe_bytes(),
                usage: BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::HostReadback,
            })
            .expect("a host-readback buffer");
        let primer: Vec<u8> = core::iter::repeat_n(poison, PROBE_ELEMENTS as usize)
            .flat_map(u32::to_le_bytes)
            .collect();
        device
            .write_buffer(prime, 0, &primer)
            .expect("a host-upload buffer is what write_buffer is for");
        Self {
            buffer,
            prime,
            staging,
            poison,
        }
    }

    /// The words the last dispatch left in this destination.
    fn read(&self, headless: &Headless) -> Vec<u32> {
        let mut bytes = poisoned(probe_bytes() as usize);
        headless.readback(self.staging, probe_bytes(), &mut bytes);
        let values: Vec<u32> = bytes
            .chunks_exact(4)
            .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
            .collect();
        // Before any caller compares it. A readback that came back short would
        // otherwise satisfy `untouched` over nothing at all, and would panic on
        // the slice in a failure message rather than saying what went wrong.
        assert_eq!(
            values.len(),
            PROBE_ELEMENTS as usize,
            "the readback is not the whole destination buffer"
        );
        values
    }

    /// Whether `values` is this destination's primer and nothing else.
    fn untouched(&self, values: &[u32]) -> bool {
        values.iter().all(|value| *value == self.poison)
    }

    fn destroy(&self, device: &dyn Device) {
        device.destroy_buffer(self.staging);
        device.destroy_buffer(self.prime);
        device.destroy_buffer(self.buffer);
    }
}

/// Primes **both** destinations, runs `record` inside a compute pass, and copies
/// both back.
///
/// Both, on one submission, every time — which is the whole design. The question
/// this exercise asks is *which* buffer a dispatch reached, and a run that
/// primed or read only the buffer it expected could not tell a dispatch that
/// went to the other one from a dispatch that never ran. The barrier shape is
/// [`ComputeProbe::run`]'s, and for its reasons.
fn update_bind_group_dispatch(
    headless: &Headless,
    first: &Destination,
    second: &Destination,
    record: impl FnOnce(&mut dyn crcbl::hal::CommandEncoder),
) -> Result<(), HalError> {
    let device = headless.device.as_ref();
    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("update bind group exercise"),
        queue: headless.queue,
    });
    let barrier = |buffer, from, to| crcbl::hal::BufferBarrier {
        buffer,
        from,
        to,
        queue_transfer: None,
    };
    let both = |from, to| {
        [
            barrier(first.buffer, from, to),
            barrier(second.buffer, from, to),
        ]
    };
    // `TransferSrc` as the source state is vacuous on the first run and is the
    // real prior use on the second — a buffer barrier carries no layout, so
    // naming a wider source scope than happened costs a stage mask and cannot be
    // wrong. [`ComputeProbe::run`] says the same at more length.
    encoder.pipeline_barrier(&Barriers {
        buffers: &both(ResourceState::TransferSrc, ResourceState::TransferDst),
        ..Barriers::default()
    });
    for destination in [first, second] {
        encoder.copy_buffer_to_buffer(&crcbl::hal::BufferCopy {
            src: destination.prime,
            src_offset: 0,
            dst: destination.buffer,
            dst_offset: 0,
            size: probe_bytes(),
        });
    }
    // `ShaderReadWrite` rather than `ShaderWrite`, for the reason
    // [`ComputeProbe::run`] gives: the barrier names what the *descriptor*
    // permits, and Slang emits no `NonReadable` for an `RWStructuredBuffer`.
    encoder.pipeline_barrier(&Barriers {
        buffers: &both(ResourceState::TransferDst, ResourceState::ShaderReadWrite),
        ..Barriers::default()
    });

    encoder.begin_compute_pass(&crcbl::hal::ComputePassDesc {
        label: Some("update bind group probe"),
        timestamp_writes: None,
    });
    record(encoder.as_mut());
    encoder.end_compute_pass();

    encoder.pipeline_barrier(&Barriers {
        buffers: &both(ResourceState::ShaderReadWrite, ResourceState::TransferSrc),
        ..Barriers::default()
    });
    for destination in [first, second] {
        encoder.copy_buffer_to_buffer(&crcbl::hal::BufferCopy {
            src: destination.buffer,
            src_offset: 0,
            dst: destination.staging,
            dst_offset: 0,
            size: probe_bytes(),
        });
    }

    let commands = encoder.finish()?;
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");
    device.wait_idle().expect("idle");
    device.destroy_command_buffer(commands);
    Ok(())
}

/// Drives [`Capability::UpdateBindGroup`] and reports whether a descriptor
/// written into a live group is the descriptor the next dispatch used.
///
/// # Why this one needs its own fixture
///
/// [`ComputeProbe`] cannot answer it twice over. Its layout carries
/// [`BindingFlags::empty()`](crcbl::hal::BindingFlags) on every entry, which
/// `crcbl-vk` refuses a rewrite against per *layout* rather than per device; and
/// it owns one destination, while the observable here is which of **two** a
/// dispatch reached. So this builds the third compute fixture in the file: the
/// same committed `compute_probe.slang` and the same three-binding layout, with
/// the flag on binding 2 and a second destination beside the first.
///
/// # What is asserted, and why a backend that does nothing cannot pass it
///
/// Two dispatches with one `update_bind_group` between them, and **both**
/// destinations primed with their own poison — [`UPDATE_POISON_FIRST`] and
/// [`UPDATE_POISON_SECOND`] — before each. The first dispatch runs with the group
/// as created, naming `first`; the update rewrites binding 2 to name `second`;
/// the second dispatch runs with nothing else changed. The four readings of that
/// second run are all distinct, which is what the two poisons buy:
///
/// * `first` still primed and `second` holding the squares — the rewrite decided
///   where the dispatch wrote. [`Exercise::Worked`].
/// * `first` holding the squares and `second` still primed — the call was
///   accepted and the dispatch went where it always went.
///   [`Exercise::SilentlyIgnored`], which is the failure the declaration exists
///   to catch.
/// * both still primed — nothing ran at all. A panic rather than an outcome: the
///   *first* dispatch is asserted before the update, so the fixture has already
///   shown this pipeline writes.
/// * anything else — the panic names which buffer disagreed and where.
///
/// # The flag is asked for only where the device can express it
///
/// [`Capability::UpdateBindGroup`] is explicit that Vulkan refuses the rewrite
/// unless the layout carries
/// [`BindingFlags::UPDATE_AFTER_BIND`](crcbl::hal::BindingFlags::UPDATE_AFTER_BIND),
/// and `crcbl_hal::pipeline`'s `check_entries` is equally explicit that *any*
/// [`BindingFlags`](crcbl::hal::BindingFlags) on a device without
/// [`Features::DESCRIPTOR_INDEXING`] must be refused rather than dropped. Those
/// two rules meet on Metal, which declares this capability `Support::Yes` and
/// withdraws `DESCRIPTOR_INDEXING` — its bind groups are flat argument tables,
/// so it can create no layout carrying the flag at all. Asking for the flag
/// unconditionally would therefore score Metal `Refused` against its own `Yes`
/// and fail a backend that performs the operation perfectly well.
///
/// So the flag is set when the device reports `DESCRIPTOR_INDEXING` and left off
/// when it does not, and each side is the strongest layout that device can
/// build. It is not a way of dodging a refusal: a Vulkan device without
/// `DESCRIPTOR_INDEXING` gets the flagless layout and `crcbl-vk` refuses the
/// rewrite against it, which is `Refused` against `Support::Yes` and fails —
/// correctly, because that device cannot do what the backend declared. No such
/// device was found here: radv and llvmpipe both report the feature.
///
/// # The update is issued between submissions, and the seam does not say whether
/// it could be issued anywhere else
///
/// Both dispatches are separate command buffers, each submitted and waited on,
/// with the rewrite in the gap. That is the only timing every backend agrees on,
/// and it is agreement rather than caution: `crcbl-mtl`'s `binding` module states
/// that "an update does not reach a command buffer that already bound the group"
/// — the values are copied onto the encoder when `bind_group` runs — so on Metal
/// a rewrite only ever shows up at the *next* bind, which is what the second
/// submission is.
///
/// What the seam says is narrower than what a caller needs, and the gap is worth
/// naming rather than working around silently.
/// [`Device::update_bind_group`](crcbl::hal::Device::update_bind_group) covers
/// exactly one timing: with `UPDATE_AFTER_BIND` the call "may be called while
/// command buffers referencing the group are pending". It says nothing about a
/// group updated while a command buffer that bound it is still *recording*, and
/// the backends would not agree if it did — `crcbl-dx12` writes descriptors
/// straight into the shader-visible heap a recorded bind already points at,
/// while `crcbl-mtl` copied the values onto the encoder and will not see the
/// write. A capability whose legal timing is unstated cannot be exercised
/// portably at that timing, so this drives the stated one.
///
/// Two further things the seam documents and two backends do not do, found while
/// writing this and left as findings rather than changed here: `crcbl-mtl` and
/// `crcbl-dx12` accept a rewrite against a layout with no `UPDATE_AFTER_BIND`,
/// where the seam documents [`HalError::Unsupported`] — and on Metal that is the
/// only kind of layout there is, so the documented rule would make its
/// `Support::Yes` describe a call that can never succeed.
///
/// # A refusal can arrive at four different points
///
/// `create_bind_group_layout` is the first, and it is where the flag is judged.
/// `create_bind_group` and `create_pipeline_layout` follow, because a backend
/// that gained the layout and not the group would refuse there.
/// `update_bind_group` itself is the fourth and is where `crcbl-wgpu` and
/// `crcbl-webgpu` land, both of them on immutable WebGPU bind groups. A backend
/// that defers its errors would refuse at
/// [`finish`](crcbl::hal::CommandEncoder::finish) instead, which
/// [`update_bind_group_dispatch`] returns rather than unwrapping. All of them
/// are [`Exercise::Refused`].
///
/// The shader module and the pipelines are the steps that panic instead: every
/// backend this suite runs already dispatches this exact artifact in
/// [`a_compute_dispatch_writes_the_values_it_was_asked_for`], so a refusal there
/// is a broken fixture rather than a capability answer.
fn exercise_update_bind_group(headless: &Headless) -> Exercise {
    let device = headless.device.as_ref();
    // `UPDATE_AFTER_BIND` is asked for where a backend will take it, because it
    // is the stronger form of this capability — the rewrite happens while the
    // group is bound. Which backends those are is **asked, not inferred**.
    //
    // It used to be inferred from `DESCRIPTOR_INDEXING`, on the reasoning that a
    // bindless-capable backend supports update-after-bind too. They are separate
    // `BindingFlags` and the heuristic broke the moment one backend had one
    // without the other: `crcbl-mtl` gained bindless, took this branch, and
    // refused the flag — copying a flat binding's values onto the encoder at
    // bind time, so a later write reaches nothing. That scored a capability the
    // backend genuinely supports as "declared supported and refused it".
    //
    // So the layout is built with the flag and rebuilt without it if the
    // backend says no, and which form ran is printed rather than left silent.
    let strong = crcbl::hal::BindingFlags::UPDATE_AFTER_BIND;
    let flags = match device.create_bind_group_layout(&crcbl::hal::BindGroupLayoutDesc {
        label: Some("update bind group flag probe"),
        entries: &[crcbl::hal::BindGroupLayoutEntry {
            binding: 0,
            visibility: crcbl::hal::ShaderStages::COMPUTE,
            kind: crcbl::hal::BindingKind::StorageBuffer {
                read_only: false,
                dynamic: false,
            },
            count: 1,
            flags: strong,
        }],
    }) {
        Ok(layout) => {
            device.destroy_bind_group_layout(layout);
            println!("crcbl hal seam e2e: update_bind_group runs the UPDATE_AFTER_BIND form");
            strong
        }
        Err(_) => {
            println!(
                "crcbl hal seam e2e: this backend refuses UPDATE_AFTER_BIND, so update_bind_group \
                 runs the plain form — the capability is the rewrite, not the flag"
            );
            crcbl::hal::BindingFlags::empty()
        }
    };

    let params = staged_buffer(
        headless,
        "update bind group params",
        &crcbl::shaders::compute_probe::Params {
            count: PROBE_ELEMENTS,
        }
        .to_bytes(),
        BufferUsage::UNIFORM,
        ResourceState::ShaderRead,
    );
    let source_bytes: Vec<u8> = probe_source()
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    let source = staged_buffer(
        headless,
        "update bind group source",
        &source_bytes,
        BufferUsage::STORAGE,
        ResourceState::ShaderRead,
    );
    let first = Destination::open(headless, "update bind group first", UPDATE_POISON_FIRST);
    let second = Destination::open(headless, "update bind group second", UPDATE_POISON_SECOND);

    // `compute_probe.slang`'s own three bindings, and the flag on the one this
    // exercise rewrites.
    let layout_entries = [
        crcbl::hal::BindGroupLayoutEntry {
            binding: 0,
            visibility: crcbl::hal::ShaderStages::COMPUTE,
            kind: crcbl::hal::BindingKind::UniformBuffer { dynamic: false },
            count: 1,
            flags: crcbl::hal::BindingFlags::empty(),
        },
        crcbl::hal::BindGroupLayoutEntry {
            binding: 1,
            visibility: crcbl::hal::ShaderStages::COMPUTE,
            kind: crcbl::hal::BindingKind::StorageBuffer {
                read_only: true,
                dynamic: false,
            },
            count: 1,
            flags: crcbl::hal::BindingFlags::empty(),
        },
        crcbl::hal::BindGroupLayoutEntry {
            binding: 2,
            visibility: crcbl::hal::ShaderStages::COMPUTE,
            kind: crcbl::hal::BindingKind::StorageBuffer {
                read_only: false,
                dynamic: false,
            },
            count: 1,
            flags,
        },
    ];

    let outcome = match device.create_bind_group_layout(&crcbl::hal::BindGroupLayoutDesc {
        label: Some("update bind group probe"),
        entries: &layout_entries,
    }) {
        Err(error) => Exercise::Refused(error),
        Ok(bind_group_layout) => {
            let group_entries = [
                crcbl::hal::BindGroupEntry {
                    binding: 0,
                    array_index: 0,
                    resource: crcbl::hal::BindingResource::whole_buffer(params),
                },
                crcbl::hal::BindGroupEntry {
                    binding: 1,
                    array_index: 0,
                    resource: crcbl::hal::BindingResource::whole_buffer(source),
                },
                crcbl::hal::BindGroupEntry {
                    binding: 2,
                    array_index: 0,
                    resource: crcbl::hal::BindingResource::whole_buffer(first.buffer),
                },
            ];
            let outcome = match device.create_bind_group(&crcbl::hal::BindGroupDesc {
                label: Some("update bind group probe"),
                layout: bind_group_layout,
                entries: &group_entries,
                variable_count: None,
            }) {
                Err(error) => Exercise::Refused(error),
                Ok(bind_group) => {
                    let set_layouts = [bind_group_layout];
                    let outcome =
                        match device.create_pipeline_layout(&crcbl::hal::PipelineLayoutDesc {
                            label: Some("update bind group probe"),
                            bind_group_layouts: &set_layouts,
                            push_constants: None,
                        }) {
                            Err(error) => Exercise::Refused(error),
                            Ok(pipeline_layout) => {
                                let outcome = update_bind_group_rewrite(
                                    headless,
                                    &first,
                                    &second,
                                    bind_group,
                                    pipeline_layout,
                                );
                                // Borrowed by the call, not taken — exactly as
                                // the bind group below is.
                                device.destroy_pipeline_layout(pipeline_layout);
                                outcome
                            }
                        };
                    device.destroy_bind_group(bind_group);
                    outcome
                }
            };
            device.destroy_bind_group_layout(bind_group_layout);
            outcome
        }
    };

    second.destroy(device);
    first.destroy(device);
    device.destroy_buffer(source);
    device.destroy_buffer(params);
    outcome
}

/// The two dispatches and the rewrite between them, once the layout, the group
/// and the pipeline layout have all been accepted.
///
/// Split out of [`exercise_update_bind_group`] so the part that is about
/// `update_bind_group` is not four `match` arms deep in the part that is about
/// building a compute pipeline. Its caller destroys everything it is handed.
fn update_bind_group_rewrite(
    headless: &Headless,
    first: &Destination,
    second: &Destination,
    bind_group: crcbl::hal::BindGroupHandle,
    pipeline_layout: crcbl::hal::PipelineLayoutHandle,
) -> Exercise {
    let device = headless.device.as_ref();
    let module = device
        .create_shader_module(&crcbl::hal::ShaderModuleDesc {
            label: Some("compute_probe.slang"),
            spirv: crcbl::shaders::COMPUTE_PROBE.spirv(),
            wgsl: crcbl::shaders::COMPUTE_PROBE.wgsl(),
            msl: crcbl::shaders::COMPUTE_PROBE.msl(),
            dxil: &crcbl::shaders::COMPUTE_PROBE.dxil_containers(),
        })
        .expect("the committed shader is accepted");
    let entry_point = crcbl::shaders::COMPUTE_PROBE
        .entry_point(crcbl::shaders::Stage::Compute)
        .expect("the probe has exactly one compute entry point");
    let pipeline = device
        .create_compute_pipeline(&crcbl::hal::ComputePipelineDesc {
            label: Some("update bind group probe"),
            layout: pipeline_layout,
            compute: crcbl::hal::ShaderEntry {
                module,
                entry_point,
            },
            workgroup_size: [crcbl::shaders::compute_probe::WORKGROUP_SIZE, 1, 1],
        })
        .expect("a compute pipeline");
    device.destroy_shader_module(module);

    let dispatch = |encoder: &mut dyn crcbl::hal::CommandEncoder| {
        encoder.bind_compute_pipeline(pipeline);
        // Inside the pass, because the open scope is the only signal the seam
        // gives the backend about which bind point a group is for.
        encoder.bind_group(0, bind_group, &[], pipeline_layout);
        encoder.dispatch(PROBE_GROUPS, 1, 1);
    };
    let squares = probe_expected(PROBE_ELEMENTS);

    let outcome = match update_bind_group_dispatch(headless, first, second, dispatch) {
        Err(error) => Exercise::Refused(error),
        Ok(()) => {
            // **The fixture asserts itself before it asserts the capability.**
            // Every reading below is "which buffer did the dispatch reach", and
            // that question is only meaningful once this dispatch is known to
            // reach one.
            assert_probe(
                &first.read(headless),
                &squares,
                "before the rewrite, the buffer the group was created naming",
            );
            let before_second = second.read(headless);
            assert!(
                second.untouched(&before_second),
                "before the rewrite, the buffer the group does not name yet already holds {:#010x?} \
                 where {:#010x} was primed. Nothing has rewritten anything at this point, so this \
                 is the fixture writing to the wrong buffer rather than a capability answer.",
                &before_second[..4],
                second.poison
            );

            match device.update_bind_group(
                bind_group,
                &[crcbl::hal::BindGroupEntry {
                    binding: 2,
                    array_index: 0,
                    resource: crcbl::hal::BindingResource::whole_buffer(second.buffer),
                }],
            ) {
                Err(error) => Exercise::Refused(error),
                Ok(()) => match update_bind_group_dispatch(headless, first, second, dispatch) {
                    Err(error) => Exercise::Refused(error),
                    Ok(()) => update_bind_group_verdict(headless, first, second, &squares),
                },
            }
        }
    };

    device.destroy_compute_pipeline(pipeline);
    outcome
}

/// Reads both destinations after the second dispatch and says which of the four
/// readings it is.
fn update_bind_group_verdict(
    headless: &Headless,
    first: &Destination,
    second: &Destination,
    squares: &[u32],
) -> Exercise {
    let after_first = first.read(headless);
    let after_second = second.read(headless);
    let first_untouched = first.untouched(&after_first);
    let second_untouched = second.untouched(&after_second);

    if first_untouched && after_second == squares {
        Exercise::Worked
    } else if second_untouched && after_first == squares {
        // The rewrite was accepted and the dispatch went where it always went.
        eprintln!(
            "crcbl hal seam e2e: after update_bind_group the dispatch still wrote the buffer the \
             group was created naming, and the buffer it was rewritten onto still holds \
             {:#010x}",
            second.poison
        );
        Exercise::SilentlyIgnored
    } else if first_untouched && second_untouched {
        panic!(
            "neither destination was written by the second dispatch: the first still holds \
             {:#010x} and the second still holds {:#010x}. The first dispatch wrote its \
             destination — that is asserted above — so this is not a pipeline that does nothing; \
             the rewrite pointed binding 2 somewhere neither of these buffers is.",
            first.poison, second.poison
        )
    } else {
        panic!(
            "the second dispatch left a reading none of the three outcomes describes. The first \
             destination begins {:#010x?} (primed with {:#010x}) and the second begins {:#010x?} \
             (primed with {:#010x}); both were expected to be either their own primer throughout \
             or the squares beginning {:#010x?}. Two destinations written, or one written \
             partially, is a rewrite that reached the descriptor and named the wrong range.",
            &after_first[..4],
            first.poison,
            &after_second[..4],
            second.poison,
            &squares[..4],
        )
    }
}

/// Drives [`Capability::PolygonModeLine`] and reports whether the triangle
/// arrived as an outline.
///
/// # What is asserted, and why a backend that does nothing cannot pass it
///
/// One triangle, one pipeline, and the only thing that separates the two answers
/// is [`PolygonMode::Line`]. The centre texel is the whole discriminator and
/// needs no threshold: [`RASTER_TRIANGLE`] contains it with room to spare, so a
/// filled triangle covers it and an outline of the same triangle does not.
///
/// * the centre is [`RASTER_BACKGROUND`] and something else was drawn — an
///   outline. [`Exercise::Worked`].
/// * the centre is [`RASTER_FIRST`] — the triangle was filled, so the fill mode
///   was accepted and ignored. [`Exercise::SilentlyIgnored`].
/// * nothing was drawn at all — a panic, because that is a broken fixture rather
///   than a wireframe: an outline of a triangle this size covers texels on every
///   rasteriser.
fn exercise_polygon_mode_line(headless: &Headless) -> Exercise {
    let device = headless.device.as_ref();
    let raster = Raster::open(headless);

    let primitive = PrimitiveState {
        polygon_mode: PolygonMode::Line,
        ..PrimitiveState::default()
    };
    let outcome = match raster.pipeline(device, "polygon mode line", primitive, None) {
        Err(error) => Exercise::Refused(error),
        Ok(pipeline) => {
            let read = raster.render(headless, "polygon mode line", None, |encoder| {
                encoder.bind_graphics_pipeline(pipeline);
                encoder.bind_group(0, raster.pair_group, &[], raster.pipeline_layout);
                encoder.draw(0..RASTER_TRIANGLE_VERTICES, 0..1);
            });
            device.destroy_graphics_pipeline(pipeline);
            match read {
                Err(error) => Exercise::Refused(error),
                Ok(read) if read.is(RASTER_FIRST) => {
                    eprintln!(
                        "crcbl hal seam e2e: the triangle came back filled — {}",
                        read.diagnosis()
                    );
                    Exercise::SilentlyIgnored
                }
                Ok(read) if read.is(RASTER_BACKGROUND) && read.drawn > 0 => {
                    eprintln!(
                        "crcbl hal seam e2e: the wireframe covered {} of {} texels and left the \
                         centre clear",
                        read.drawn,
                        RASTER_WIDTH * RASTER_HEIGHT
                    );
                    Exercise::Worked
                }
                Ok(read) if read.drawn == 0 => panic!(
                    "the pass drew nothing at all, so this says nothing about the fill mode: an \
                     outline of a triangle this size lights texels on every rasteriser.\n  {}",
                    read.diagnosis()
                ),
                Ok(read) => panic!(
                    "the wireframe pass left a colour this exercise never draws: {}",
                    read.diagnosis()
                ),
            }
        }
    };

    raster.destroy(device);
    outcome
}

/// Drives [`Capability::DepthClamp`] and reports whether a primitive past the
/// far plane survived to the target.
///
/// # What is asserted, and why a backend that does nothing cannot pass it
///
/// The triangle sits at [`RASTER_DEPTH_BEYOND_FAR`] with `w = 1`, so it is
/// entirely outside the clip volume in Z and inside it in X and Y. Clipping
/// discards it; clamping is defined as replacing that clip with a clamp of the
/// fragment's depth, so it survives. The pass carries no depth attachment at
/// all, which keeps the reading unambiguous: there is no depth *test* that could
/// be the reason a fragment vanished, only the clip.
///
/// * [`RASTER_BEYOND`] in the centre — the primitive was clamped rather than
///   clipped. [`Exercise::Worked`].
/// * [`RASTER_BACKGROUND`] — it was clipped, so `depth_clamp` was accepted and
///   ignored. [`Exercise::SilentlyIgnored`].
fn exercise_depth_clamp(headless: &Headless) -> Exercise {
    let device = headless.device.as_ref();
    let raster = Raster::open(headless);

    let primitive = PrimitiveState {
        depth_clamp: true,
        ..PrimitiveState::default()
    };
    let outcome = match raster.pipeline(device, "depth clamp", primitive, None) {
        Err(error) => Exercise::Refused(error),
        Ok(pipeline) => {
            let read = raster.render(headless, "depth clamp", None, |encoder| {
                encoder.bind_graphics_pipeline(pipeline);
                encoder.bind_group(0, raster.beyond_group, &[], raster.pipeline_layout);
                encoder.draw(0..RASTER_TRIANGLE_VERTICES, 0..1);
            });
            device.destroy_graphics_pipeline(pipeline);
            match read {
                Err(error) => Exercise::Refused(error),
                Ok(read) if read.is(RASTER_BEYOND) => Exercise::Worked,
                Ok(read) if read.is(RASTER_BACKGROUND) => {
                    eprintln!(
                        "crcbl hal seam e2e: the primitive at z={RASTER_DEPTH_BEYOND_FAR} was \
                         clipped, so depth clamping was not applied — {}",
                        read.diagnosis()
                    );
                    Exercise::SilentlyIgnored
                }
                Ok(read) => panic!(
                    "the depth-clamp pass left a colour this exercise never draws: {}",
                    read.diagnosis()
                ),
            }
        }
    };

    raster.destroy(device);
    outcome
}

/// The `u64` a query resolve destination holds before the resolve.
///
/// [`FILL_POISON`]'s job, one type wider. A resolve that was accepted and
/// dropped leaves this behind, so "the destination was never written" stays
/// distinguishable from "the queries really did read back these numbers"
/// instead of being folded into it. No counter a device hands out reaches this
/// value within a process's lifetime, so it cannot be mistaken for a timestamp
/// either.
const QUERY_POISON: u64 = 0x5A5A_5A5A_5A5A_5A5A;

/// Queries the timestamp exercise writes: a pair bracketing a **render** pass,
/// then a pair bracketing a **compute** one.
///
/// **Two passes rather than one, because the capability covers two paths.**
/// Since timestamps moved into the pass descriptor they live on
/// [`RenderPassDesc`](crcbl::hal::RenderPassDesc) *and*
/// [`ComputePassDesc`](crcbl::hal::ComputePassDesc), and on `crcbl-webgpu` those
/// are two different stream commands carrying two different pairs of queries,
/// decoded separately in `web/engine/gpu-replay.js`. A render-only exercise left
/// that second encoding untested. `crcbl-vk` and `crcbl-dx12` funnel both kinds
/// through one `open_pass_timestamps`, so for them this is the same path twice —
/// which is a fine thing for a test to prove rather than assume.
const TIMESTAMP_QUERIES: u32 = 4;

/// Where the compute pass's pair sits in the set, after the render pass's.
const TIMESTAMP_COMPUTE_FIRST: u32 = 2;

/// Bytes one resolved query occupies, and so the stride
/// [`resolve_query_set`](crcbl::hal::CommandEncoder::resolve_query_set) writes
/// into its destination.
const QUERY_RESULT_BYTES: u64 = size_of::<u64>() as u64;

/// Side of the square attachment the timed render pass clears.
///
/// The seam times a **pass** and nothing else, so the work between the two
/// timestamps has to be work a pass can do. A clear is the one such workload
/// that needs no pipeline, no shader and no vertex data, which is what makes it
/// the same workload on every backend — and at this size it writes
/// [`TIMED_CLEAR_BYTES`], far more than a device's counter granularity, so a
/// second timestamp above the first is a fact about the hardware rather than a
/// race with the clock. Nothing ever reads these texels: the clear is here for
/// the time it takes.
const TIMED_CLEAR_EXTENT: u32 = 1024;

/// Bytes [`TIMED_CLEAR_EXTENT`] of [`Format::Rgba8Unorm`] covers.
const TIMED_CLEAR_BYTES: u64 =
    TIMED_CLEAR_EXTENT as u64 * TIMED_CLEAR_EXTENT as u64 * size_of::<u32>() as u64;

/// Longest a clear of [`TIMED_CLEAR_BYTES`] may be reported to have taken.
///
/// One second, which is not a performance bound — the software rasterisers this
/// suite runs on in CI are entitled to be slow, and a loaded machine slower
/// still. It is a **unit** bound, and enormously loose on purpose: the mistakes
/// it exists to catch are off by six orders of magnitude or more (a period
/// applied as a frequency, a microsecond count reported as nanoseconds), so a
/// number a real device could reach cannot also be one of them.
const TIMED_CLEAR_CEILING_NANOS: u64 = 1_000_000_000;

/// How many queries the creation-only exercise asks a set to hold.
///
/// More than one, so that reading one past the end has an in-range read beside
/// it to be compared against.
const CREATED_QUERIES: u32 = 2;

/// Drives a [`QueryKind::Timestamp`] set through both of the seam's read paths
/// and reports whether what came back could have come from a clock.
///
/// # What is asserted, and why a backend that does nothing cannot pass it
///
/// A render pass clears a [`TIMED_CLEAR_EXTENT`]-square attachment with a
/// [`PassTimestampWrites`] naming both of its boundaries, and the closing
/// timestamp must be **strictly greater** than the opening one. That is the one
/// relationship every API's timestamps obey without knowing the device:
/// Vulkan's `vkCmdWriteTimestamp2` and D3D12's `EndQuery` both record a raw
/// tick whose scale is `VkPhysicalDeviceLimits::timestampPeriod` or
/// `ID3D12CommandQueue::GetTimestampFrequency`, Metal's
/// `MTLCounterSampleBuffer` records an `MTLTimestamp`, and WebGPU's
/// `timestampWrites` records nanoseconds outright — four units, one shared
/// property, and [`Device::query_results`] reports all four in the same unit —
/// **nanoseconds** — because each backend converts. A magic number would be a
/// different assertion on every device; this is the same assertion on all of
/// them.
///
/// Three failures it cannot be green through. Timestamp writes accepted and
/// dropped — which the seam explicitly permits on a device *without*
/// [`Features::TIMESTAMP_QUERY`], and which is therefore exactly what a wrong
/// `Support::Yes` looks like — read back two zeros. A counter that is not
/// running reads back the same value twice. A resolve that writes nothing
/// leaves [`QUERY_POISON`] in the destination. None of the three satisfies
/// `end > start` against a written destination.
///
/// **The clear is what makes the strict inequality safe.** An empty pass is two
/// adjacent writes, which a device with a coarse counter is entitled to report
/// as the same tick; a pass that writes [`TIMED_CLEAR_BYTES`] is not.
///
/// **The magnitude is checked too, because the unit is half the claim**, and
/// only in one direction — which is worth saying plainly. The seam reports
/// nanoseconds, so [`TIMED_CLEAR_CEILING_NANOS`] catches a conversion that went
/// the wrong way: `crcbl-dx12` multiplying by `GetTimestampFrequency` instead of
/// dividing puts a 4 MiB clear at eighteen orders of magnitude too long, and
/// nothing about "the clock moved" would notice.
///
/// There is **no cross-backend floor**, and there cannot be one here. A backend
/// that forgot to convert and handed the seam its raw ticks under-reports by
/// exactly its period — ten on the radv Navi 31 this workspace tests against,
/// and *one* on a device whose tick already is a nanosecond, where the two are
/// the same number. So no bound this exercise can state distinguishes them on
/// every device. What does is a suite that knows the device:
/// `crcbl-vk`'s `per_pass_gpu_timers_report_real_numbers` holds a real frame to
/// a floor, and `crcbl-dx12`'s
/// `d3d12_timestamps_advance_and_both_read_paths_report_the_same_ticks` converts
/// the resolve's ticks itself and demands the read match.
///
/// # Why the results are read before they are resolved
///
/// [`Device::query_results`] runs first and the resolve into a buffer only
/// afterwards, because a resolve *waits* for availability — `crcbl-vk` issues
/// `vkCmdCopyQueryPoolResults` with `VK_QUERY_RESULT_WAIT_BIT` — and a query
/// that was reset and never written never becomes available. Reading the
/// non-blocking path first means a backend whose pass timestamps are a silent
/// no-op fails this exercise rather than hanging the suite inside it.
fn exercise_timestamp_query(headless: &Headless) -> Exercise {
    let device = headless.device.as_ref();
    let set = match device.create_query_set(&QuerySetDesc {
        label: Some("timestamp exercise"),
        kind: QueryKind::Timestamp,
        count: TIMESTAMP_QUERIES,
    }) {
        Ok(set) => set,
        Err(error) => return Exercise::Refused(error),
    };

    let target = device
        .create_image(&ImageDesc {
            label: Some("timed clear target"),
            image_type: ImageType::D2,
            extent: Extent3d::d2(TIMED_CLEAR_EXTENT, TIMED_CLEAR_EXTENT),
            format: RASTER_FORMAT,
            mip_levels: 1,
            samples: 1,
            usage: ImageUsage::COLOR_ATTACHMENT,
        })
        .expect("a colour attachment");
    let view = device
        .create_image_view(&ImageViewDesc {
            label: Some("timed clear view"),
            image: target,
            view_type: ImageViewType::D2,
            format: RASTER_FORMAT,
            range: ImageSubresourceRange::all(RASTER_FORMAT),
        })
        .expect("a view of a colour attachment");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("timestamp exercise"),
        queue: headless.queue,
    });
    encoder.pipeline_barrier(&Barriers {
        images: &[ImageBarrier::new(
            target,
            ImageSubresourceRange::all(RASTER_FORMAT),
            ResourceState::Undefined,
            ResourceState::ColorAttachment,
        )],
        ..Barriers::default()
    });
    // Always reset before writing: Vulkan requires it and every other backend
    // accepts it, which is why the seam has callers do it unconditionally.
    encoder.reset_query_set(set, 0..TIMESTAMP_QUERIES);
    // The timed work, and the only shape the seam can time: a pass. Nothing is
    // drawn — the clear is the workload — so this needs no pipeline and is the
    // same measurement on a backend that has no shader compiled.
    encoder.begin_render_pass(&crcbl::hal::RenderPassDesc {
        label: Some("timed clear"),
        color_attachments: &[ColorAttachment {
            view,
            resolve: None,
            load: LoadOp::Clear,
            store: StoreOp::Store,
            clear: ClearValue::color([0.25, 0.5, 0.75, 1.0]),
        }],
        depth_stencil_attachment: None,
        render_area: Rect2d::from_size(TIMED_CLEAR_EXTENT, TIMED_CLEAR_EXTENT),
        timestamp_writes: Some(crcbl::hal::PassTimestampWrites {
            set,
            beginning_of_pass: 0,
            end_of_pass: 1,
        }),
    });
    encoder.end_render_pass();

    // **The second path the capability covers.** Empty on purpose: what is
    // being tested is that the *pass descriptor's* two queries were carried,
    // encoded and written — not how long a dispatch takes. An empty pass is
    // also why the assertion below allows the two ticks to be equal where the
    // render pass's must strictly increase: two writes with nothing between
    // them can land in one tick of a coarse counter, which is a property of the
    // clock rather than a lost write, and a zero is what a lost write leaves.
    encoder.begin_compute_pass(&crcbl::hal::ComputePassDesc {
        label: Some("timed empty compute pass"),
        timestamp_writes: Some(crcbl::hal::PassTimestampWrites {
            set,
            beginning_of_pass: TIMESTAMP_COMPUTE_FIRST,
            end_of_pass: TIMESTAMP_COMPUTE_FIRST + 1,
        }),
    });
    encoder.end_compute_pass();

    let outcome = match encoder.finish() {
        Err(error) => Exercise::Refused(error),
        Ok(commands) => {
            device
                .submit(headless.queue, &SubmitInfo::new(&[commands]))
                .expect("submit");
            device.wait_idle().expect("idle");
            device.destroy_command_buffer(commands);

            // Primed rather than zeroed: a query nothing wrote does not
            // reliably read back as zero — vk returns whatever the pool held —
            // so zero is not the sentinel it looks like. `QUERY_POISON` is a
            // value no clock produces, which makes "this query was never
            // written" distinguishable from "this query holds a tick".
            let mut nanos = [QUERY_POISON; TIMESTAMP_QUERIES as usize];
            device
                .query_results(set, 0, &mut nanos)
                .expect("reading a set this exercise created, over the range it created it with");
            let [start, end, compute_start, compute_end] = nanos;
            if start == 0 && end == 0 {
                // The documented degrade for a device without the feature, and
                // therefore what a wrong `Support::Yes` produces.
                Exercise::SilentlyIgnored
            } else {
                assert!(
                    end > start,
                    "the timestamps bracketing a pass that cleared {TIMED_CLEAR_BYTES} bytes \
                     came back as {start} then {end}. A clock that ran cannot report the closing \
                     write at or before the opening one, so either the writes did not land where \
                     they were asked to or the counter behind them is not moving."
                );
                let elapsed = end - start;
                assert!(
                    elapsed < TIMED_CLEAR_CEILING_NANOS,
                    "clearing {TIMED_CLEAR_BYTES} bytes is reported as {elapsed} nanoseconds. \
                     The seam's unit is nanoseconds and no device is that slow, so this is a \
                     conversion applied the wrong way round rather than a slow machine."
                );
                eprintln!(
                    "crcbl hal seam e2e: the timed clear took {micros:.3} µs",
                    micros = elapsed as f64 / 1_000.0,
                );

                // The compute pass's pair, held to the same claim as the render
                // pass's except for the tie: a zero is a query nothing wrote, and
                // on a backend where the two pass kinds are separate encodings —
                // `crcbl-webgpu` carries them as different stream commands — this
                // is the only thing that reads the second one.
                // Both sentinels, because which one an unwritten query keeps is
                // the backend's business: vk zeroes the whole read when any
                // query in the range is unavailable — measured, it zeroes the
                // render pair too, which the branch above catches first — while
                // a backend that wrote the render pair and dropped this one
                // leaves either a zero or the poison this read was primed with.
                // That asymmetric case is the one this exists for, since
                // `crcbl-webgpu` encodes the two pass kinds separately.
                assert!(
                    !(compute_start == 0 && compute_end == 0)
                        && compute_start != QUERY_POISON
                        && compute_end != QUERY_POISON,
                    "the render pass wrote {start} then {end} and the compute pass left \
                     {compute_start} and {compute_end} — still the poison this read was primed \
                     with, so nothing wrote those two queries. The pass descriptor carried them \
                     and something between there and the device dropped them, which on a backend \
                     encoding compute and render passes separately is a whole path failing while \
                     the other passes."
                );
                assert!(
                    compute_end >= compute_start,
                    "the compute pass opened at {compute_start} and closed at {compute_end}, \
                     which is a clock running backwards or two queries read in the wrong order. \
                     Equal is allowed here and not for the render pass, because this pass is \
                     empty and two writes with nothing between them may share a tick."
                );
                eprintln!(
                    "crcbl hal seam e2e: the empty compute pass spanned {} ns",
                    compute_end - compute_start,
                );
                // The refusal that keeps the pair meaningful, on a backend that
                // has just shown it takes the pair at all. A pass writing both
                // ends into one query measures nothing anywhere — WebGPU
                // requires the two to be distinct outright — and every backend
                // implements the check, but until now only `crcbl-hal`'s null
                // backend asserted it: vk's could be deleted with all seventeen
                // of these tests green.
                let mut coincident = device.create_command_encoder(&CommandEncoderDesc {
                    label: Some("timestamps into one query"),
                    queue: headless.queue,
                });
                coincident.reset_query_set(set, 0..TIMESTAMP_QUERIES);
                coincident.begin_render_pass(&crcbl::hal::RenderPassDesc {
                    label: Some("both ends into query 0"),
                    color_attachments: &[ColorAttachment {
                        view,
                        resolve: None,
                        load: LoadOp::Clear,
                        store: StoreOp::Store,
                        clear: ClearValue::color([0.25, 0.5, 0.75, 1.0]),
                    }],
                    depth_stencil_attachment: None,
                    render_area: Rect2d::from_size(TIMED_CLEAR_EXTENT, TIMED_CLEAR_EXTENT),
                    timestamp_writes: Some(crcbl::hal::PassTimestampWrites {
                        set,
                        beginning_of_pass: 0,
                        end_of_pass: 0,
                    }),
                });
                coincident.end_render_pass();
                let refusal = coincident.finish();
                assert!(
                    matches!(refusal, Err(HalError::InvalidDescriptor(_))),
                    "a pass writing both timestamps into one query must be refused as \
                     InvalidDescriptor — the indices are numbers the caller can correct, and a \
                     pass that measures nothing is worse than one that refuses: {:?}",
                    refusal.map(|_| "recorded successfully")
                );

                exercise_timestamp_resolve(headless, set, nanos)
            }
        }
    };

    device.destroy_image_view(view);
    device.destroy_image(target);
    device.destroy_query_set(set);
    outcome
}

/// The second half of [`exercise_timestamp_query`]: the same two queries, read
/// again through [`resolve_query_set`](crcbl::hal::CommandEncoder::resolve_query_set).
///
/// The destination is primed with [`QUERY_POISON`] first, so a resolve that was
/// accepted and dropped is `SilentlyIgnored` rather than an unexplained
/// mismatch.
///
/// # The two paths are in different units, so they are held to a *ratio*
///
/// [`Device::query_results`] reports nanoseconds and `resolve_query_set` writes
/// the device's own values — it is a GPU-side copy with nothing to convert
/// with, which the seam documents on both calls. So the second read cannot be
/// compared to `nanos` for equality, and it is not compared to a plausible
/// range either: one scale relates every pair, so
/// `nanos[i] * raw[j] == nanos[j] * raw[i]` for all of them, whatever that
/// scale is. That is what a backend reading a different pool, a different range
/// or a different stride cannot produce — its numbers have no scale relating
/// them to the ones already returned — and it holds identically on a backend
/// where the scale is one.
///
/// The products are taken in `u128`: a timestamp is a full `u64` off a
/// free-running counter, so two of them multiplied leave `u64` at once.
///
/// This cannot block: `nanos` is only reached because the queries have already
/// been observed as available.
fn exercise_timestamp_resolve(
    headless: &Headless,
    set: QuerySetHandle,
    nanos: [u64; TIMESTAMP_QUERIES as usize],
) -> Exercise {
    const BYTES: u64 = TIMESTAMP_QUERIES as u64 * QUERY_RESULT_BYTES;
    let device = headless.device.as_ref();

    let prime = device
        .create_buffer(&BufferDesc {
            label: Some("timestamp resolve prime"),
            size: BYTES,
            usage: BufferUsage::TRANSFER_SRC,
            memory: MemoryLocation::HostUpload,
        })
        .expect("a host-upload buffer");
    let resolved = device
        .create_buffer(&BufferDesc {
            label: Some("timestamp resolve"),
            size: BYTES,
            usage: BufferUsage::QUERY_RESOLVE
                | BufferUsage::TRANSFER_DST
                | BufferUsage::TRANSFER_SRC,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("a device-local buffer");
    let staging = device
        .create_buffer(&BufferDesc {
            label: Some("timestamp resolve readback"),
            size: BYTES,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a host-readback buffer");

    let primer: Vec<u8> = core::iter::repeat_n(QUERY_POISON, TIMESTAMP_QUERIES as usize)
        .flat_map(u64::to_le_bytes)
        .collect();
    device
        .write_buffer(prime, 0, &primer)
        .expect("a host-upload buffer is what write_buffer is for");

    let barrier = |buffer, from, to| crcbl::hal::BufferBarrier {
        buffer,
        from,
        to,
        queue_transfer: None,
    };
    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("timestamp resolve"),
        queue: headless.queue,
    });
    encoder.pipeline_barrier(&Barriers {
        buffers: &[barrier(
            resolved,
            ResourceState::Undefined,
            ResourceState::TransferDst,
        )],
        ..Barriers::default()
    });
    encoder.copy_buffer_to_buffer(&crcbl::hal::BufferCopy {
        src: prime,
        src_offset: 0,
        dst: resolved,
        dst_offset: 0,
        size: BYTES,
    });
    encoder.pipeline_barrier(&Barriers {
        buffers: &[barrier(
            resolved,
            ResourceState::TransferDst,
            ResourceState::TransferDst,
        )],
        ..Barriers::default()
    });
    encoder.resolve_query_set(set, 0..TIMESTAMP_QUERIES, resolved, 0);
    encoder.pipeline_barrier(&Barriers {
        buffers: &[barrier(
            resolved,
            ResourceState::TransferDst,
            ResourceState::TransferSrc,
        )],
        ..Barriers::default()
    });
    encoder.copy_buffer_to_buffer(&crcbl::hal::BufferCopy {
        src: resolved,
        src_offset: 0,
        dst: staging,
        dst_offset: 0,
        size: BYTES,
    });

    let outcome = match encoder.finish() {
        Err(error) => Exercise::Refused(error),
        Ok(commands) => {
            device
                .submit(headless.queue, &SubmitInfo::new(&[commands]))
                .expect("submit");
            device.wait_idle().expect("idle");
            device.destroy_command_buffer(commands);

            let mut bytes = poisoned(BYTES as usize);
            headless.readback(staging, BYTES, &mut bytes);
            let words: Vec<u64> = bytes
                .chunks_exact(8)
                .map(|word| u64::from_le_bytes(word.try_into().expect("eight bytes")))
                .collect();
            if words.iter().all(|word| *word == QUERY_POISON) {
                Exercise::SilentlyIgnored
            } else {
                // The anchor is the query with the largest raw reading, so the
                // cross-multiplication is never against a zero — a pair where
                // both sides are zero relates to everything and would make this
                // vacuous.
                let anchor = (0..words.len())
                    .max_by_key(|index| words[*index])
                    .expect("the resolve wrote at least one word");
                for index in 0..words.len() {
                    // `nanos[i] = round(raw[i] * scale)` on both sides, so the
                    // cross products differ by at most half a nanosecond of raw
                    // on each — hence the tolerance, which is the two readings
                    // themselves rather than a number picked to fit.
                    let left = u128::from(nanos[anchor]) * u128::from(words[index]);
                    let right = u128::from(nanos[index]) * u128::from(words[anchor]);
                    let slack = u128::from(words[anchor]) + u128::from(words[index]);
                    assert!(
                        left.abs_diff(right) <= slack,
                        "resolve_query_set wrote {words:?} for the same four queries \
                         Device::query_results had just read as {nanos:?} nanoseconds. Query \
                         {index} does not sit on the same scale as query {anchor}, so one of the \
                         two read paths is reaching a different pool, a different range or a \
                         different stride."
                    );
                }
                Exercise::Worked
            }
        }
    };

    device.destroy_buffer(staging);
    device.destroy_buffer(resolved);
    device.destroy_buffer(prime);
    outcome
}

/// Drives what the seam actually offers for a [`QueryKind::Occlusion`] or
/// [`QueryKind::PipelineStatistics`] set: creating one, recording against it,
/// and reading it.
///
/// # This deliberately asserts nothing about a count
///
/// Both kinds count something a *draw* produces — occlusion counts samples that
/// passed the depth test, pipeline statistics count invocations and primitives
/// — and every API scopes them with a begin/end pair around that draw:
/// `vkCmdBeginQuery`/`vkCmdEndQuery`, D3D12's `BeginQuery`/`EndQuery`, Metal's
/// `setVisibilityResultMode:offset:` on a render encoder, WebGPU's
/// `beginOcclusionQuery` inside a render pass with an `occlusionQuerySet`
/// attached. **[`crcbl::hal::CommandEncoder`] has no such verb** — its query
/// vocabulary is `reset_query_set`, `resolve_query_set` and the pass
/// descriptor's `timestamp_writes` —
/// so no work this suite can record ever reaches these pools, and there is no
/// number to be plausible about. Asserting on a resolve of them would be
/// asserting on queries that were never begun: the value is whatever "not
/// written" resolves to, which is zero on the backend that has them, and zero
/// is also what a backend doing nothing produces.
///
/// The missing verb is the whole of it, and it is worth saying that the *work*
/// is no longer the obstacle: [`Raster`] draws a triangle of a known size, which
/// is what an occlusion count and `crcbl-vk`'s `VERTEX_SHADER_INVOCATIONS |
/// FRAGMENT_SHADER_INVOCATIONS | CLIPPING_PRIMITIVES` statistics pool would
/// count. Neither can be scoped around it from this seam.
///
/// # What it does assert
///
/// The refusal, in full: a backend declaring these unsupported must fail
/// `create_query_set` with [`HalError::Unsupported`], which is the direction
/// four of the five backends are in and the direction that was previously
/// unchecked.
///
/// And, for a backend declaring support, that the handle reaches the command
/// stream: the set is reset through a submitted command buffer, so a backend
/// handing out a handle its own encoder does not recognise fails at `finish`.
/// Vulkan needs that reset before any read besides, since a pool that was never
/// reset may not be read at all.
///
/// [`QueryKind::Occlusion`] then gets a read on top, which shows the handle
/// names a pool of the size that was asked for rather than a stub:
/// [`Device::query_results`] over `0..count` must succeed *and* a read one query
/// past the end must be refused with [`HalError::InvalidDescriptor`]. The
/// **pair** is the check — `Ok` alone is what an implementation doing nothing
/// answers to everything, and it is the refusal beside it that makes the
/// success mean something.
///
/// [`QueryKind::PipelineStatistics`] gets no read, and the reason is a seam
/// defect rather than a shortcut. Both of the seam's read paths assume one
/// `u64` per query — [`Device::query_results`] takes `out.len()` results
/// starting at `first_query`, and `crcbl-vk` resolves with a `size_of::<u64>()`
/// stride — while a Vulkan statistics pool holds one `u64` per *enabled
/// counter*, and `crcbl-vk` enables three. Asking for two queries hands
/// `vkGetQueryPoolResults` 16 bytes where it needs 32, which the validation
/// layer catches as `VUID-vkGetQueryPoolResults-dataSize-00817`. There is no
/// `out` length that both satisfies Vulkan and passes the seam's own
/// `first_query + out.len() <= count` bound, so a statistics pool cannot be read
/// through this seam at all: that wants a seam change and is left named here
/// rather than papered over with a read that only looks like one.
fn exercise_query_set_creation(headless: &Headless, kind: QueryKind) -> Exercise {
    let device = headless.device.as_ref();
    let set = match device.create_query_set(&QuerySetDesc {
        label: Some("query set exercise"),
        kind,
        count: CREATED_QUERIES,
    }) {
        Ok(set) => set,
        Err(error) => return Exercise::Refused(error),
    };

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("query set exercise"),
        queue: headless.queue,
    });
    encoder.reset_query_set(set, 0..CREATED_QUERIES);

    let outcome = match encoder.finish() {
        Err(error) => Exercise::Refused(error),
        Ok(commands) => {
            device
                .submit(headless.queue, &SubmitInfo::new(&[commands]))
                .expect("submit");
            device.wait_idle().expect("idle");
            device.destroy_command_buffer(commands);

            if kind == QueryKind::PipelineStatistics {
                // One `u64` per query is not this pool's layout; see the note
                // above on `VUID-vkGetQueryPoolResults-dataSize-00817`.
                Exercise::Worked
            } else {
                let mut inside = [0u64; CREATED_QUERIES as usize];
                device
                    .query_results(set, 0, &mut inside)
                    .unwrap_or_else(|error| {
                        panic!(
                            "{kind:?}: this backend created a {CREATED_QUERIES}-query set and then \
                             refused to read queries 0..{CREATED_QUERIES} of it — {error}. Either \
                             the handle names no pool or this exercise is broken; it is not a \
                             capability refusal, which create_query_set is where it belongs."
                        )
                    });

                let mut past_the_end = [0u64; CREATED_QUERIES as usize + 1];
                match device.query_results(set, 0, &mut past_the_end) {
                    Err(HalError::InvalidDescriptor(_)) => Exercise::Worked,
                    Ok(()) => Exercise::SilentlyIgnored,
                    Err(error) => panic!(
                        "{kind:?}: reading {} queries out of a {CREATED_QUERIES}-query set was \
                         refused with {error}, and the seam documents InvalidDescriptor for a \
                         range that exceeds the set. A caller cannot tell an over-long read from a \
                         dead handle if both answer the same way.",
                        past_the_end.len()
                    ),
                }
            }
        }
    };

    device.destroy_query_set(set);
    outcome
}

// --- the bindless path -----------------------------------------------------

/// The word every element of the bindless destination holds before the dispatch.
///
/// [`FILL_POISON`]'s job for this exercise, and a different value from it, from
/// [`PUSH_CONSTANT_POISON`] and from every word the source buffers hold, so that
/// "this destination was never written" stays separable from "the shader wrote
/// it" and from "some other exercise's primer reached it".
const BINDLESS_POISON: u32 = 0xB0B0_0B0B;

/// Descriptors the array binding declares room for, against the
/// `SOURCE_COUNT` the bind group actually uses.
///
/// **The shader's own capacity, and it has to be exactly that.**
/// `bindless_probe.slang` declares a sized array inside a `ParameterBlock` —
/// Metal refuses an unbounded one outright — and Vulkan's rule for a sized array
/// is that it be no larger than its binding's `descriptorCount`. A layout
/// declaring fewer would describe a shader other than the one dispatched below,
/// so this is read from the shader rather than chosen here.
///
/// It is still a *ceiling*:
/// [`BindingFlags::VARIABLE_COUNT`](crcbl::hal::BindingFlags::VARIABLE_COUNT)
/// makes the layout's count the upper bound and
/// [`BindGroupDesc::variable_count`](crcbl::hal::BindGroupDesc) the length the
/// group chooses inside it, and the group below chooses `SOURCE_COUNT`. A
/// ceiling equal to the length would be satisfied by a backend that ignored the
/// field and sized the array from the layout, which is exactly the fixed-array
/// downgrade this capability separates out — the two assertions under this are
/// what keep them apart.
const BINDLESS_CEILING: u32 = crcbl::shaders::bindless_probe::SOURCE_CAPACITY;

const _: () = assert!(
    BINDLESS_CEILING > crcbl::shaders::bindless_probe::SOURCE_COUNT,
    "the layout's ceiling and the group's length are the same number, so a backend that ignored \
     variable_count and sized the array from its layout would pass this exercise"
);

const _: () = assert!(
    // Strictly less, which on integers is `BINDLESS_CEILING + 1 <=` and is the
    // spelling the linter leaves alone: the one buffer of headroom is the
    // destination's, in the set beside this one.
    BINDLESS_CEILING < crcbl::hal::PORTABLE_STORAGE_BUFFERS_PER_STAGE,
    "the array's ceiling plus the destination is more storage buffers in one stage than a WebGPU \
     device guarantees, so this layout is one a portable backend may refuse for a reason that has \
     nothing to do with bindless"
);

/// Word `word` of source buffer `source`, as the caller writes it and as the
/// shader must read it back.
///
/// **Distinct across every source *and* every word**, which is what makes the
/// two failure modes separable. A backend that bound the array's first
/// descriptor for every index — the flat-argument-table shape — returns source
/// 0's block four times over; one that read each buffer at the wrong offset
/// returns the right source with its words rotated. A constant per buffer would
/// hide the second and a constant everywhere would hide both.
///
/// The high byte is a fixed tag so that no word can collide with
/// [`BINDLESS_POISON`] or be zero, which the two `SilentlyIgnored` arms below
/// depend on.
const fn bindless_word(source: u32, word: u32) -> u32 {
    0xBD00_0000 | (source << 8) | word
}

// The premise both `SilentlyIgnored` arms rest on, checked at compile time
// because every side is a constant. A source word equal to the poison would make
// a dispatch that never ran read back as one that worked, and a source word of
// zero would do the same for an array whose descriptors delivered nothing.
const _: () = {
    let mut source = 0;
    while source < crcbl::shaders::bindless_probe::SOURCE_COUNT {
        let mut word = 0;
        while word < crcbl::shaders::bindless_probe::WORDS_PER_SOURCE {
            assert!(
                bindless_word(source, word) != BINDLESS_POISON,
                "a source word equals BINDLESS_POISON, so a dispatch that never ran and one that \
                 read the array are no longer distinguishable"
            );
            assert!(
                bindless_word(source, word) != 0,
                "a source word is zero, so an array that delivered nothing and one that was read \
                 are no longer distinguishable"
            );
            word += 1;
        }
        source += 1;
    }
};

/// What the destination must hold when every descriptor was read from the buffer
/// the group put in it.
fn bindless_expected() -> Vec<u32> {
    use crcbl::shaders::bindless_probe::{SOURCE_COUNT, WORDS_PER_SOURCE};
    (0..SOURCE_COUNT)
        .flat_map(|source| (0..WORDS_PER_SOURCE).map(move |word| bindless_word(source, word)))
        .collect()
}

/// Drives [`Capability::BindlessDescriptorArray`](crcbl::hal::Capability::BindlessDescriptorArray)
/// and reports whether the words a shader read out of a descriptor array are the
/// words the caller put in its descriptors.
///
/// # Why this one needs its own fixture, and its own shader
///
/// Nothing else here can answer it. Every other layout in this file is a fixed
/// list of scalar bindings, and — until `bindless_probe.slang` — no committed
/// artifact declared a resource array at all: every `Texture2DArray` in
/// `crcbl-shaders` is one layered image rather than an array of descriptors, no
/// committed SPIR-V declared `RuntimeDescriptorArray`, and no WGSL contained
/// `binding_array`. So the capability was `Unexercised` while every backend
/// declared an answer for it.
///
/// The layout is the capability's own definition: a
/// [`BindingFlags::VARIABLE_COUNT`](crcbl::hal::BindingFlags::VARIABLE_COUNT)
/// entry declaring a [`BINDLESS_CEILING`] it does not fill, and a
/// [`BindGroupDesc::variable_count`](crcbl::hal::BindGroupDesc) choosing the
/// length inside it. It is the only entry of its own bind group, because the
/// shader declares the array inside a `ParameterBlock` and Slang gives such a
/// block a descriptor set to itself — so this exercise builds **two** groups,
/// the destination at set 0 and the table at set 1, and the array is trivially
/// both the last entry and the highest binding number
/// `crcbl_hal::BindGroupLayoutDesc::check_entries` requires it to be.
///
/// # What is asserted, and why a backend that does nothing cannot pass it
///
/// The destination is primed with [`BINDLESS_POISON`] through a copy from a host
/// buffer — a copy rather than a `fill_buffer`, for the reason
/// [`ComputeProbe::run`] gives — and each of the array's descriptors gets a
/// **different** buffer holding [`bindless_word`]'s pattern. Then one workgroup
/// per descriptor is dispatched and the destination is read back:
///
/// * the pattern — every descriptor was read from its own buffer, at the right
///   offsets. [`Exercise::Worked`].
/// * [`BINDLESS_POISON`] throughout — the prime landed and the dispatch produced
///   nothing at all. [`Exercise::SilentlyIgnored`].
/// * zeros throughout — the prime was overwritten, so the shader ran, and every
///   descriptor it read delivered nothing. [`Exercise::SilentlyIgnored`] as
///   well, because both are the seam accepting a request and changing nothing
///   observable.
/// * anything else — the array was bound wrongly, and the panic names the first
///   element that disagreed and what the two most likely shapes look like.
///
/// # A refusal can arrive at four different points
///
/// `crcbl-mtl` **used to** refuse every
/// [`BindingFlags`](crcbl::hal::BindingFlags) here and no longer does: it binds
/// the table as a Metal argument buffer of device addresses, and refuses only
/// the flags it genuinely cannot serve. `crcbl-webgpu` refuses because WebGPU
/// has no binding arrays at all; and
/// `crcbl-wgpu` refuses there too, because a wgpu binding array's length is the
/// layout's fixed count rather than something a group chooses. A backend that
/// accepted the layout and not the group would refuse at `create_bind_group` —
/// which is where all three also refuse `variable_count` — and one that defers
/// its errors would refuse at [`finish`](crcbl::hal::CommandEncoder::finish), so
/// every step is matched rather than the first alone.
///
/// The shader module is the one step that panics instead, for
/// [`exercise_push_constants_on_compute`]'s reason: it is reached only after the
/// layout was accepted, which no backend without descriptor arrays gets to.
fn exercise_bindless_descriptor_array(headless: &Headless) -> Exercise {
    use crcbl::shaders::bindless_probe::{DESTINATION_BYTES, SOURCE_BYTES, SOURCE_COUNT};

    let device = headless.device.as_ref();

    let prime = device
        .create_buffer(&BufferDesc {
            label: Some("bindless prime"),
            size: DESTINATION_BYTES,
            usage: BufferUsage::TRANSFER_SRC,
            memory: MemoryLocation::HostUpload,
        })
        .expect("a host-upload buffer");
    let primer: Vec<u8> = core::iter::repeat_n(BINDLESS_POISON, bindless_expected().len())
        .flat_map(u32::to_le_bytes)
        .collect();
    device
        .write_buffer(prime, 0, &primer)
        .expect("a host-upload buffer is what write_buffer is for");

    let destination = device
        .create_buffer(&BufferDesc {
            label: Some("bindless destination"),
            size: DESTINATION_BYTES,
            usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_DST | BufferUsage::TRANSFER_SRC,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("a device-local buffer");
    let staging = device
        .create_buffer(&BufferDesc {
            label: Some("bindless readback"),
            size: DESTINATION_BYTES,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a host-readback buffer");

    // One buffer per descriptor, each with its own contents. Distinct buffers
    // rather than one buffer at four offsets: two descriptors resolving to the
    // same object would let a backend that dropped one of them still look
    // correct, which is `crcbl-wgpu`'s array suite's reason for the same choice.
    let upload = device
        .create_buffer(&BufferDesc {
            label: Some("bindless source upload"),
            size: SOURCE_BYTES * u64::from(SOURCE_COUNT),
            usage: BufferUsage::TRANSFER_SRC,
            memory: MemoryLocation::HostUpload,
        })
        .expect("a host-upload buffer");
    let source_bytes: Vec<u8> = bindless_expected()
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect();
    device
        .write_buffer(upload, 0, &source_bytes)
        .expect("write");
    let sources: Vec<crcbl::hal::BufferHandle> = (0..SOURCE_COUNT)
        .map(|index| {
            device
                .create_buffer(&BufferDesc {
                    label: Some(&format!("bindless source {index}")),
                    size: SOURCE_BYTES,
                    usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_DST,
                    memory: MemoryLocation::DeviceLocal,
                })
                .expect("a device-local buffer")
        })
        .collect();

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("bindless upload"),
        queue: headless.queue,
    });
    for (index, source) in sources.iter().enumerate() {
        encoder.copy_buffer_to_buffer(&crcbl::hal::BufferCopy {
            src: upload,
            src_offset: SOURCE_BYTES * index as u64,
            dst: *source,
            dst_offset: 0,
            size: SOURCE_BYTES,
        });
    }
    let barriers: Vec<crcbl::hal::BufferBarrier> = sources
        .iter()
        .map(|source| crcbl::hal::BufferBarrier {
            buffer: *source,
            from: ResourceState::TransferDst,
            to: ResourceState::ShaderRead,
            queue_transfer: None,
        })
        .collect();
    encoder.pipeline_barrier(&Barriers {
        buffers: &barriers,
        ..Barriers::default()
    });
    let commands = encoder.finish().expect("recording succeeded");
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");
    device.wait_idle().expect("idle");
    device.destroy_command_buffer(commands);
    device.destroy_buffer(upload);

    // **Two layouts, because the shader's array is a `ParameterBlock`.** Slang
    // gives such a block a descriptor set of its own, so the committed
    // artifacts put `destination` at set 0 binding 0 and the table at set 1
    // binding 0 — `crcbl_shaders::bindless_probe` records it per target. The
    // array is therefore the only entry of its own layout, which satisfies
    // `check_entries`' rule that a `VARIABLE_COUNT` entry be both last in its
    // slice and its highest binding number without the two competing for one
    // set.
    let destination_entries = [crcbl::hal::BindGroupLayoutEntry {
        binding: 0,
        visibility: crcbl::hal::ShaderStages::COMPUTE,
        kind: crcbl::hal::BindingKind::StorageBuffer {
            read_only: false,
            dynamic: false,
        },
        count: 1,
        flags: crcbl::hal::BindingFlags::empty(),
    }];
    let table_entries = [crcbl::hal::BindGroupLayoutEntry {
        binding: 0,
        visibility: crcbl::hal::ShaderStages::COMPUTE,
        kind: crcbl::hal::BindingKind::StorageBuffer {
            read_only: true,
            dynamic: false,
        },
        count: BINDLESS_CEILING,
        // `VARIABLE_COUNT` alone. `PARTIALLY_BOUND` would say the shader may
        // read a descriptor the group never wrote, and this group writes
        // every descriptor it declares — declaring it would widen what a
        // backend is allowed to leave uninitialised without widening
        // anything this exercise observes, and reading an unwritten
        // descriptor is undefined rather than wrong.
        flags: crcbl::hal::BindingFlags::VARIABLE_COUNT,
    }];
    // The destination's layout carries no `BindingFlags` at all, so a backend
    // that refuses this capability refuses the *second* one — which is why the
    // first is not matched: a failure there would be a plain storage buffer
    // being refused, which every other exercise in this file would have caught
    // first.
    let destination_layout = device
        .create_bind_group_layout(&crcbl::hal::BindGroupLayoutDesc {
            label: Some("bindless destination"),
            entries: &destination_entries,
        })
        .expect("a layout holding one storage buffer");
    let outcome = match device.create_bind_group_layout(&crcbl::hal::BindGroupLayoutDesc {
        label: Some("bindless probe"),
        entries: &table_entries,
    }) {
        Err(error) => Exercise::Refused(error),
        Ok(table_layout) => {
            let destination_group = device
                .create_bind_group(&crcbl::hal::BindGroupDesc {
                    label: Some("bindless destination"),
                    layout: destination_layout,
                    entries: &[crcbl::hal::BindGroupEntry {
                        binding: 0,
                        array_index: 0,
                        resource: crcbl::hal::BindingResource::whole_buffer(destination),
                    }],
                    variable_count: None,
                })
                .expect("a group holding one storage buffer");
            let table_entries: Vec<crcbl::hal::BindGroupEntry> = sources
                .iter()
                .enumerate()
                .map(|(index, source)| crcbl::hal::BindGroupEntry {
                    binding: 0,
                    // The whole bindless write path in one field: one entry per
                    // descriptor, each naming which element of the array it
                    // fills.
                    array_index: index as u32,
                    resource: crcbl::hal::BindingResource::whole_buffer(*source),
                })
                .collect();
            let outcome = match device.create_bind_group(&crcbl::hal::BindGroupDesc {
                label: Some("bindless probe"),
                layout: table_layout,
                entries: &table_entries,
                // Below `BINDLESS_CEILING`: the length is chosen here, which is
                // what the capability is.
                variable_count: Some(SOURCE_COUNT),
            }) {
                Err(error) => Exercise::Refused(error),
                Ok(table_group) => {
                    let outcome = bindless_dispatch(
                        headless,
                        [destination_layout, table_layout],
                        [destination_group, table_group],
                        prime,
                        destination,
                        staging,
                    );
                    device.destroy_bind_group(table_group);
                    outcome
                }
            };
            device.destroy_bind_group(destination_group);
            device.destroy_bind_group_layout(table_layout);
            outcome
        }
    };
    device.destroy_bind_group_layout(destination_layout);

    for source in sources {
        device.destroy_buffer(source);
    }
    device.destroy_buffer(staging);
    device.destroy_buffer(destination);
    device.destroy_buffer(prime);
    outcome
}

/// Builds the pipeline over both set layouts, binds both groups, dispatches one
/// workgroup per bound descriptor, and says which of the outcomes the readback
/// is.
///
/// `set_layouts` and `bind_groups` are in set order — the destination at 0 and
/// the descriptor table at 1 — because that is the order a pipeline layout
/// numbers them in and the order the shader's artifacts declare.
///
/// Split out of [`exercise_bindless_descriptor_array`] so that the buffers and
/// the descriptor array — which is what that function is about — are not
/// interleaved with a pipeline every other compute exercise here also builds.
fn bindless_dispatch(
    headless: &Headless,
    set_layouts: [crcbl::hal::BindGroupLayoutHandle; 2],
    bind_groups: [crcbl::hal::BindGroupHandle; 2],
    prime: crcbl::hal::BufferHandle,
    destination: crcbl::hal::BufferHandle,
    staging: crcbl::hal::BufferHandle,
) -> Exercise {
    use crcbl::shaders::bindless_probe::{DESTINATION_BYTES, SOURCE_COUNT, WORKGROUP_SIZE};

    let device = headless.device.as_ref();
    // Set 0 then set 1, in the order the artifacts number them: the
    // destination, then the descriptor table the `ParameterBlock` moved into a
    // set of its own.
    let pipeline_layout = match device.create_pipeline_layout(&crcbl::hal::PipelineLayoutDesc {
        label: Some("bindless probe"),
        bind_group_layouts: &set_layouts,
        push_constants: None,
    }) {
        Err(error) => return Exercise::Refused(error),
        Ok(pipeline_layout) => pipeline_layout,
    };

    let module = device
        .create_shader_module(&crcbl::hal::ShaderModuleDesc {
            label: Some("bindless_probe.slang"),
            spirv: crcbl::shaders::BINDLESS_PROBE.spirv(),
            // `wgsl` is `None`, and that is the artifact rather than an
            // omission: `crcbl-webgpu` and `crcbl-wgpu` refuse this layout
            // above and could never reach here. The MSL is real — the shader's
            // array is a bounded `ParameterBlock` precisely so that Metal has
            // something loadable — and this line is now taken, because
            // `crcbl-mtl` binds the table as an argument buffer.
            wgsl: crcbl::shaders::BINDLESS_PROBE.wgsl(),
            msl: crcbl::shaders::BINDLESS_PROBE.msl(),
            dxil: &crcbl::shaders::BINDLESS_PROBE.dxil_containers(),
        })
        .expect(
            "this backend accepted a VARIABLE_COUNT layout and then refused the committed \
             bindless_probe artifact — that is a fixture problem rather than a capability answer",
        );
    let entry_point = crcbl::shaders::BINDLESS_PROBE
        .entry_point(crcbl::shaders::Stage::Compute)
        .expect("the probe has exactly one compute entry point");
    let outcome = match device.create_compute_pipeline(&crcbl::hal::ComputePipelineDesc {
        label: Some("bindless probe"),
        layout: pipeline_layout,
        compute: crcbl::hal::ShaderEntry {
            module,
            entry_point,
        },
        workgroup_size: [WORKGROUP_SIZE, 1, 1],
    }) {
        Err(error) => Exercise::Refused(error),
        Ok(pipeline) => {
            let read = primed_dispatch(
                headless,
                prime,
                destination,
                staging,
                DESTINATION_BYTES,
                |e| {
                    e.bind_compute_pipeline(pipeline);
                    for (set, group) in bind_groups.iter().enumerate() {
                        e.bind_group(set as u32, *group, &[], pipeline_layout);
                    }
                    // One workgroup per bound descriptor: the shader takes its
                    // array index from `SV_GroupID`, which is the dynamically
                    // uniform value descriptor indexing is defined for.
                    e.dispatch(SOURCE_COUNT, 1, 1);
                },
            );
            device.destroy_compute_pipeline(pipeline);
            match read {
                Err(error) => Exercise::Refused(error),
                Ok(words) => bindless_outcome(&words),
            }
        }
    };
    device.destroy_shader_module(module);
    device.destroy_pipeline_layout(pipeline_layout);
    outcome
}

/// Which outcome a bindless readback is.
fn bindless_outcome(words: &[u32]) -> Exercise {
    let want = bindless_expected();
    if words == want.as_slice() {
        return Exercise::Worked;
    }
    if words.iter().all(|word| *word == BINDLESS_POISON) {
        eprintln!(
            "crcbl hal seam e2e: the destination still holds {BINDLESS_POISON:#010x} throughout, \
             so the dispatch wrote nothing at all"
        );
        return Exercise::SilentlyIgnored;
    }
    if words.iter().all(|word| *word == 0) {
        eprintln!(
            "crcbl hal seam e2e: the dispatch overwrote the prime with zeros, so the shader ran \
             and every descriptor it read through the array delivered nothing"
        );
        return Exercise::SilentlyIgnored;
    }

    use crcbl::shaders::bindless_probe::WORDS_PER_SOURCE;
    let index = words
        .iter()
        .zip(&want)
        .position(|(got, expected)| got != expected)
        .expect("the two agree word for word only on the arm above");
    let block = index as u32 / WORDS_PER_SOURCE;
    let flat = words
        .chunks_exact(WORDS_PER_SOURCE as usize)
        .all(|chunk| chunk == &want[..WORDS_PER_SOURCE as usize]);
    panic!(
        "element {index} of the destination came back {:#010x} where {:#010x} was written into \
         descriptor {block}'s own buffer. The dispatch left {words:#010x?} against \
         {want:#010x?}.\n  Every word here came out of the descriptor array and landed somewhere \
         the array did not put it, which is a wrong picture with a clean log. \
         {}",
        words[index],
        want[index],
        if flat {
            "Every block holds the FIRST descriptor's words, which is the flat-argument-table \
             shape: the array's index was dropped and one descriptor answered for all of them."
        } else {
            "The blocks differ from each other, so the array was indexed — a descriptor was \
             filled from the wrong buffer, or a buffer was read at the wrong offset."
        },
    )
}

/// Drives [`Capability::StorageImageBinding`](crcbl::hal::Capability::StorageImageBinding):
/// a layout holding [`BindingKind::StorageImage`](crcbl::hal::BindingKind::StorageImage)
/// entries, and a pipeline layout over it.
///
/// **A LAYOUT AND NOT A DISPATCH, WHICH IS WHAT THE CAPABILITY SAYS IT IS.** Its
/// own words are "a `BindingKind::StorageImage` entry in a bind group layout …
/// so the layout cannot be built at all, rather than the binding merely being
/// slower" — the refusal it exists to describe arrives at layout creation, on
/// every backend that has one, and a shader that writes through the binding
/// would be observing something the capability does not claim. No committed
/// artifact declares a storage image in any case: neither the `.slang` sources
/// nor the WGSL the WebGPU path carries has one, so a dispatch here would need a
/// fifth shader before it could assert anything.
///
/// **The pipeline layout is what stops this asserting that a handle came back.**
/// `create_bind_group_layout` on a backend that streams its commands answers
/// `Ok` without a device having seen the descriptor, so building the layout
/// alone would be the "every return value said success" shape the declaration
/// test exists to catch. A pipeline layout is where D3D12 serialises the root
/// signature the descriptor ranges went into and where Vulkan and Metal size
/// their tables, so it is the first call that must read the entry to succeed.
///
/// **Two entries, so neither new field is dead.** One writable `D2` slot and one
/// read-only `D2Array` one, in two different formats — a mapping that dropped
/// the `view_type` or the `format` still builds this layout, but a backend that
/// went on to disagree with the bound view about either would have nothing here
/// to disagree *with* if both entries were the same.
///
/// [`Format::Rgba8Unorm`] and [`Format::Rgba16Float`] are the two formats every
/// backend allows as a storage texture, WebGPU included — its list excludes
/// every sRGB, depth and block-compressed format, and the seam's remaining
/// colour formats are gated behind WebGPU features `crcbl_hal::Features` has no
/// bit to ask for.
fn exercise_storage_image_binding(headless: &Headless) -> Exercise {
    let device = headless.device.as_ref();
    let entries = [
        crcbl::hal::BindGroupLayoutEntry {
            binding: 0,
            visibility: crcbl::hal::ShaderStages::COMPUTE,
            kind: crcbl::hal::BindingKind::StorageImage {
                read_only: false,
                view_type: ImageViewType::D2,
                format: Format::Rgba8Unorm,
            },
            count: 1,
            flags: crcbl::hal::BindingFlags::empty(),
        },
        crcbl::hal::BindGroupLayoutEntry {
            binding: 1,
            visibility: crcbl::hal::ShaderStages::COMPUTE,
            kind: crcbl::hal::BindingKind::StorageImage {
                read_only: true,
                view_type: ImageViewType::D2Array,
                format: Format::Rgba16Float,
            },
            count: 1,
            flags: crcbl::hal::BindingFlags::empty(),
        },
    ];

    let layout = match device.create_bind_group_layout(&crcbl::hal::BindGroupLayoutDesc {
        label: Some("storage image binding exercise"),
        entries: &entries,
    }) {
        Ok(layout) => layout,
        // The capability's refusal is `Unsupported` and nothing else. An
        // `InvalidDescriptor` here would be this exercise writing a layout the
        // seam's own rules reject, which is a broken test rather than a backend
        // declining a capability — and reporting it as a refusal would let a
        // `Support::No` be satisfied by a typo.
        Err(error @ HalError::Unsupported { .. }) => return Exercise::Refused(error),
        Err(error) => panic!(
            "a two-entry storage-image layout was refused with {error}, and \
             Capability::StorageImageBinding is declared through HalError::Unsupported. Either \
             this exercise builds a layout the seam rejects for an unrelated reason, or the \
             backend is reporting a capability refusal under the wrong error."
        ),
    };

    let set_layouts = [layout];
    let outcome = match device.create_pipeline_layout(&crcbl::hal::PipelineLayoutDesc {
        label: Some("storage image binding exercise"),
        bind_group_layouts: &set_layouts,
        push_constants: None,
    }) {
        Ok(pipeline_layout) => {
            device.destroy_pipeline_layout(pipeline_layout);
            Exercise::Worked
        }
        Err(error @ HalError::Unsupported { .. }) => Exercise::Refused(error),
        Err(error) => panic!(
            "the bind group layout was built and the pipeline layout over it was refused with \
             {error}. A backend that accepts a storage-image entry and then cannot put it in a \
             root signature or an argument table has declared something it does not do."
        ),
    };

    device.destroy_bind_group_layout(layout);
    outcome
}

/// **A `SamplerDesc` with anisotropy above `1.0`** — which is what
/// [`Capability::SamplerAnisotropy`](crcbl::hal::Capability::SamplerAnisotropy)
/// says it is, and the whole of it.
///
/// # This is narrower than it looks, and that is the point
///
/// It was unexercised for a long time on the reasoning that "the observable is a
/// filtered texel", needing a shader that samples a minified texture at a
/// grazing angle. That argues against a *different* capability — one about
/// filtering quality — and the enum does not define this one that way. It is the
/// same correction [`exercise_storage_image_binding`] came out of: the row was
/// about the layout, not about a shader reading it.
///
/// # What actually makes this a check rather than "a handle came back"
///
/// Three things, and none of them needs a texel:
///
/// * **The limit has to agree with the claim.** A backend declaring support must
///   report a [`Limits::max_sampler_anisotropy`](crcbl::hal::Limits) above
///   `1.0`; one is the value that *disables* anisotropy, so a device claiming the
///   capability and capping it at one has claimed something it cannot do. That
///   is a disagreement between two seam surfaces, and it is exactly the shape a
///   backend gets wrong by reporting a feature bit it never wired to a limit.
/// * **Validation is the observer for the accepting case.** This suite runs
///   under the Vulkan validation layers in CI, where creating a sampler with
///   `anisotropyEnable` on a device that did not enable `samplerAnisotropy` is
///   an error rather than a silent pass — so a backend that shrugs and hands back
///   a handle it should have refused is caught by the layer, not by an
///   assertion here that could not see it.
/// * **The refusing case is checked in full.** A backend declaring no support
///   must refuse the descriptor, and the driver holds it to
///   [`HalError::Unsupported`] like every other row.
///
/// What it still does not assert is that the sampler *filters* anisotropically.
/// That is a golden's job and it is one no backend-agnostic suite can do without
/// a committed texture-sampling artifact per backend; see the module header.
fn exercise_sampler_anisotropy(headless: &Headless) -> Exercise {
    let device = headless.device.as_ref();
    let limit = device.caps().limits.max_sampler_anisotropy;

    // Asked for at the device's own cap rather than at a number written here: the
    // descriptor documents itself as capped by that limit, so a constant would be
    // testing this suite's guess about the hardware on any device that reports
    // less.
    let sampler = match device.create_sampler(&SamplerDesc {
        label: Some("anisotropy exercise"),
        anisotropy: limit,
        ..SamplerDesc::default()
    }) {
        Ok(sampler) => sampler,
        Err(error) => return Exercise::Refused(error),
    };
    device.destroy_sampler(sampler);

    // The cross-check, and it runs only on the accepting path because a backend
    // that refused has already answered.
    if limit <= 1.0 {
        eprintln!(
            "crcbl hal seam e2e: the sampler was created, but this device caps anisotropy at \
             {limit}, and 1.0 is the value that disables it — so nothing above 1.0 was actually \
             asked for"
        );
        return Exercise::SilentlyIgnored;
    }
    Exercise::Worked
}

/// Drives `capability` against this device, or says why it cannot be.
///
/// **Exhaustive with no wildcard arm**, so a capability added to
/// [`crcbl::hal::Capability`] has to be given an exercise or an explicit reason
/// it has none. That is what stops the coverage half of this mechanism rotting
/// the way the capabilities themselves did: a new behaviour cannot arrive here
/// as a silence.
#[deny(clippy::wildcard_enum_match_arm)]
fn exercise(headless: &Headless, capability: crcbl::hal::Capability) -> Exercise {
    use crcbl::hal::Capability as C;

    // Every reason below names what an exercise would need, not merely that
    // there isn't one — an unexercised capability is a coverage gap, and a gap
    // nobody can act on is worse than one nobody wrote down.
    const NEEDS_MESH_ARTIFACTS: &str = "needs committed mesh/task shader artifacts per backend, which crcbl-vk and crcbl-dx12 \
         have and crcbl-mtl and crcbl-webgpu do not. But the obstacle is no longer only the \
         artifacts: crcbl-dx12 creates the pipeline and records both draws — it is drawn on WARP \
         by its own suite — while still declaring Support::No, because it does not report \
         Features::MESH_SHADER. This capability is defined as the pipeline being creatable and \
         the draws recordable, so an exercise here would score dx12 as declaring unsupported \
         something it performs, and fail. Adding it therefore waits on the DX12 mesh reporting \
         slice rather than on an artifact";
    match capability {
        C::BufferFillZero => exercise_clear(headless),
        C::ImageToImageCopy => exercise_image_to_image_copy(headless),
        C::DepthImageCopy => exercise_depth_image_copy(headless),
        // A clear, not a draw: this one needs no pipeline at all, which is why
        // it was driven before the raster fixture below existed.
        C::MsaaResolveAttachment => exercise_msaa_resolve(headless),
        C::StencilReference => exercise_stencil_reference(headless),
        C::DrawIndirectCount => exercise_draw_indirect_count(headless),
        C::IndirectArgumentPaddedStride => exercise_indirect_argument_padded_stride(headless),
        C::MeshShading | C::TaskShaderStage => Exercise::Unexercised(NEEDS_MESH_ARTIFACTS),
        // The one that left the unexercised list most recently, and it took a
        // new shader to do it: `bindless_probe.slang` is the first committed
        // artifact in the tree to declare an array of descriptors at all.
        C::BindlessDescriptorArray => exercise_bindless_descriptor_array(headless),
        // The one that left that pair, and it left because the capability is
        // narrower than the reason was: it is about the layout, not about a
        // shader reading it. See `exercise_storage_image_binding`.
        C::StorageImageBinding => exercise_storage_image_binding(headless),
        // The one that left that group: `compute_probe.slang`'s layout is fixed,
        // but nothing said it had to be built once — this one is built again
        // with UPDATE_AFTER_BIND on its destination and a second destination
        // beside it, which is all the capability needs to be observable.
        C::UpdateBindGroup => exercise_update_bind_group(headless),
        // The one before it: `push_constant_probe.slang` is a second compute
        // fixture whose only input besides its one binding is the block.
        C::PushConstants => exercise_push_constants(headless),
        C::PolygonModeLine => exercise_polygon_mode_line(headless),
        C::DepthClamp => exercise_depth_clamp(headless),
        // The one rasteriser capability the raster fixture does not reach, and
        // it is a shader gap rather than a pipeline one.
        C::SamplerAnisotropy => exercise_sampler_anisotropy(headless),
        C::TimestampQuery => exercise_timestamp_query(headless),
        // Creation and refusal only, and `exercise_query_set_creation` says at
        // length why there is no count to assert: the seam has no begin/end
        // verb, so nothing this suite records ever reaches either pool.
        C::OcclusionQuery => exercise_query_set_creation(headless, QueryKind::Occlusion),
        C::PipelineStatisticsQuery => {
            exercise_query_set_creation(headless, QueryKind::PipelineStatistics)
        }
        C::TimelineSemaphore => exercise_timeline(headless, TimelineClaim::Counter),
        C::CpuTimelineWait => exercise_timeline(headless, TimelineClaim::CpuWait),
        // **Not covered by the timeline test**, which creates only a
        // `SemaphoreKind::Timeline` — the enum keeps the two apart precisely
        // because a device with no timeline must still hand out a binary one,
        // and the timeline test would pass either way.
        C::BinarySemaphore => Exercise::Unexercised(
            "the observable claim is ordering between two submissions, and on a one-queue \
             backend a dropped binary semaphore is indistinguishable from an honoured one \
             because submission order already provides it; creation alone would assert that a \
             handle came back and nothing about what it does",
        ),
        C::CpuTimelineSignal => exercise_cpu_timeline_signal(headless),
        // The one that left the unexercised list, and it left because
        // `Device::signal_semaphore` arrived: the wait is satisfied from the
        // test thread instead of by a second submission the queue can never
        // reach. `exercise_timeline_wait_before_signal` says how the sequence is
        // ordered so that a backend which cannot do it refuses before anything
        // is submitted, rather than stopping the queue for the rest of the run.
        C::TimelineWaitBeforeSignal => exercise_timeline_wait_before_signal(headless),
    }
}

/// **A backend does what it says it does.** Both directions, per capability.
///
/// The half the compiler cannot check. `Device::supports` is an exhaustive
/// `match`, so every backend has an *answer*; nothing in the type system makes
/// the answer true. Here a declared capability must actually work and a declared
/// refusal must actually refuse — a backend that claims a fill it drops fails,
/// and so does one that quietly performs something it declared absent.
///
/// A capability with no exercise is reported by name, not skipped: the run
/// prints what it covered and what it did not, so "this passed" never silently
/// means "this asked nothing".
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-hal-seam-e2e.sh"]
fn every_declared_capability_behaves_the_way_it_was_declared() {
    use crcbl::hal::{Capability, Support};

    let headless = Headless::open_with_every_optional_feature();
    let backend = headless.device.backend();

    let mut driven = 0usize;
    let mut unexercised: Vec<String> = Vec::new();
    let mut failures: Vec<String> = Vec::new();

    for capability in Capability::ALL {
        let declared = headless.device.supports(*capability);
        let outcome = exercise(&headless, *capability);
        match (&declared, &outcome) {
            // The reason is printed, not counted. A coverage gap nobody can act
            // on is worse than one nobody wrote down, so the run has to carry
            // what an exercise would take rather than a tally.
            (_, Exercise::Unexercised(why)) => {
                unexercised.push(format!("{capability} ({why})"));
            }
            (Support::Yes, Exercise::Worked)
            | (Support::No(_) | Support::NotOnThisDevice(_), Exercise::Refused(_)) => {
                driven += 1;
            }
            (Support::Yes, Exercise::Refused(error)) => failures.push(format!(
                "{capability}: {backend} declares it supported and refused it — {error}"
            )),
            (Support::Yes, Exercise::SilentlyIgnored) => failures.push(format!(
                "{capability}: {backend} declares it supported, accepted the call and changed \
                 nothing. This is the failure the declaration exists to catch: every return value \
                 said success."
            )),
            // **Both refusals, held to the same standard.** A
            // `NotOnThisDevice` is the device's no rather than the backend's,
            // and it is still a no: a backend that blames the device and then
            // performs the call has declared something untrue about the device.
            (Support::No(why) | Support::NotOnThisDevice(why), Exercise::Worked) => {
                failures.push(format!(
                    "{capability}: {backend} declares it unsupported ({why}) and then performed \
                     it. Either the declaration is stale or something else answered."
                ))
            }
            (Support::No(why) | Support::NotOnThisDevice(why), Exercise::SilentlyIgnored) => {
                failures.push(format!(
                    "{capability}: {backend} declares it unsupported ({why}) and dropped the call \
                     silently instead of refusing it. The seam requires a loud refusal — a dropped \
                     request is a wrong picture with a clean log."
                ))
            }
        }

        // **A capability refusal is `HalError::Unsupported` and nothing else.**
        // That is the variant a caller matches on to choose a fallback path, so
        // a backend answering `InvalidDescriptor` — "your descriptor is wrong",
        // which sends a caller looking for a field to correct — is invisible to
        // exactly the branch the variant exists for. This accepted either while
        // `crcbl-mtl` and `crcbl-dx12` still did that; both now refuse by
        // capability, so the looser arm would only let it come back.
        //
        // `InvalidDescriptor` is still right elsewhere, and the exercises are
        // written so it cannot arrive here: `exercise_fill` fills a range inside
        // a buffer it just created, so a refusal is about the *value*. An
        // `InvalidHandle` or a lost device means this exercise broke rather than
        // the backend refused, which would make every `No` above pass for the
        // wrong reason.
        if let Exercise::Refused(error) = &outcome {
            assert!(
                matches!(error, HalError::Unsupported { .. }),
                "{capability}: {backend} refused with {error}, which is not the \
                 HalError::Unsupported the seam documents for \"this backend cannot\". A caller \
                 branching on that variant to pick a fallback would miss this refusal entirely — \
                 and an InvalidHandle or a lost device here means this exercise is broken, not \
                 the backend."
            );
        }
    }

    eprintln!(
        "crcbl hal seam e2e: capability coverage on {backend}: {driven} driven, {} unexercised of \
         {}. Not exercised here:\n  {}",
        unexercised.len(),
        Capability::ALL.len(),
        unexercised.join("\n  "),
    );
    // The mismatches first, and the vacuity guard second. A backend that drops
    // every exercised call has *both* problems at once, and reporting "nothing
    // was driven" would hide the twenty lines saying which calls were dropped —
    // which is what happened the first time this order was the other way round.
    assert!(
        failures.is_empty(),
        "{} of {backend}'s capability declarations do not match what it does:\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
    assert!(
        driven > 0,
        "no capability was driven at all, so this test asserted nothing about {backend}"
    );

    headless.finish();
}

/// **Divergence is deliberate or it is a bug.** The parity report.
///
/// Every capability this backend refuses must be accounted for. The backend's
/// own refusal — `Support::No` — needs a `crcbl_hal::DIVERGENCES` row with a
/// written reason, **on every device**. A refusal the *device* caused —
/// `Support::NotOnThisDevice`, which only `Support::granted` hands out — needs
/// nothing, because `Features` already models a lesser device and logs it as a
/// downgrade at creation.
///
/// And the converse, which is what stops the list becoming a museum: a
/// capability the list says this backend lacks, and which it turns out to have,
/// fails until the entry is deleted.
///
/// # The third outcome, and why it is printed rather than failed
///
/// A pair the device withheld the gate for is `UnprovableHere`: this run learned
/// nothing about the backend either way, so failing would be demanding hardware
/// nobody has. It is named in the report all the same, because a coverage gap
/// that reads as a clean pass is how this mechanism used to rot — the rule
/// waived *every* refusal on a device reporting the flag clear, including
/// refusals the backend made on its own account, and so retired their rows for
/// them. Eight of `crcbl-mtl`'s nine rows were that shape.
///
/// The whole matrix is printed either way, so a CI log carries the report rather
/// than only the verdict.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-hal-seam-e2e.sh"]
fn the_parity_report_matches_the_reviewed_divergence_list() {
    use crcbl::hal::{Capability, ParityVerdict, divergence, parity_verdict};

    let headless = Headless::open_with_every_optional_feature();
    let backend = headless.device.backend();
    let features = headless.device.caps().features;

    let mut unreviewed: Vec<String> = Vec::new();
    let mut stale: Vec<String> = Vec::new();
    let mut unprovable: Vec<String> = Vec::new();
    let mut report = String::new();

    for capability in Capability::ALL {
        let declared = headless.device.supports(*capability);
        let verdict = parity_verdict(*capability, backend, declared, features);
        report.push_str(&format!("\n  {capability:<30} {declared}"));

        match verdict {
            ParityVerdict::Supported => {
                if let Some(entry) = divergence(*capability, backend) {
                    stale.push(format!(
                        "{capability}: DIVERGENCES says {backend} lacks it — {} — and this device \
                         has it. Delete the entry.",
                        entry.why
                    ));
                }
            }
            ParityVerdict::Reviewed(entry) => {
                report.push_str(&format!("  [reviewed: {}]", entry.why));
            }
            // Named, counted and printed — never silently skipped. What the run
            // could not settle is as much of the report as what it could.
            ParityVerdict::UnprovableHere(gate) => {
                report.push_str(&format!(
                    "  [unprovable here: this device withheld {gate:?}]"
                ));
                unprovable.push(format!(
                    "{capability} (this device withheld {gate:?}, so {backend} was never asked)"
                ));
            }
            // **The failure a retired row lands on.** Deliberately says what
            // evidence a retirement takes, because the next person to read this
            // is whoever deleted the row.
            ParityVerdict::Unreviewed => unreviewed.push(format!(
                "{capability}: {backend} refuses it on its own account ({}) and no DIVERGENCES \
                 entry says why. If a row was just deleted, put it back: retiring one takes \
                 {backend} answering Support::Yes here and this suite driving the call — not a \
                 device that happened to withhold {:?}, which proves nothing about the backend \
                 either way.",
                declared.reason().unwrap_or("no reason given"),
                capability.gating_feature()
            )),
            // The one arm a backend could hide a refusal behind, so it is
            // checked against the device rather than believed.
            ParityVerdict::FalseDeviceGate => unreviewed.push(format!(
                "{capability}: {backend} answered Support::NotOnThisDevice ({}) and this device \
                 withheld nothing — the capability is gated on {:?} and the device reports \
                 {:?}. Support::granted is the only honest source of that answer; a hand-written \
                 one is a refusal wearing the device's name.",
                declared.reason().unwrap_or("no reason given"),
                capability.gating_feature(),
                features,
            )),
        }
    }

    eprintln!("crcbl hal seam e2e: capability parity on {backend}:{report}");
    eprintln!(
        "crcbl hal seam e2e: {} of {} capabilities were unprovable on this device:\n  {}",
        unprovable.len(),
        Capability::ALL.len(),
        if unprovable.is_empty() {
            "(none — every capability was either performed or refused by the backend itself)"
                .to_string()
        } else {
            unprovable.join("\n  ")
        }
    );
    assert!(
        unreviewed.is_empty(),
        "{} unreviewed divergence(s) on {backend}:\n  {}",
        unreviewed.len(),
        unreviewed.join("\n  ")
    );
    assert!(
        stale.is_empty(),
        "{} stale DIVERGENCES entr(ies) for {backend}:\n  {}",
        stale.len(),
        stale.join("\n  ")
    );

    // **No vacuity guard here, deliberately.** The obvious one — "not every
    // capability was unprovable" — is a green light wired to nothing: an
    // *ungated* capability can never be `UnprovableHere`, because a backend
    // claiming the device withheld something with no gating feature is
    // `FalseDeviceGate` and fails above. Eleven of the capabilities are ungated,
    // so the count this would compare has a ceiling it cannot reach.
    //
    // What keeps the run honest instead is that both directions above are held
    // for every capability that is not unprovable, and the tally line names the
    // ones that are — so a device that answered for nothing says so in the log
    // rather than passing quietly. `driven > 0` in
    // `every_declared_capability_behaves_the_way_it_was_declared` is the
    // vacuity guard that can actually fail, and it is that test's.
    headless.finish();
}
