//! The presenter against the null backend: the adapter walk, the composite
//! alpha, the swapchain, the frame ring, the pacing and the device's own
//! failures, each injected through the backend's recorder.

use super::*;

use crate::window::wait_for_configure;
use crcbl_shell::HeadlessShell;

// The backend walk past a backend that refuses the window.
mod backend_walk;

// Frame requests pacing the presents.
mod frame_gate;

/// A headless shell and one window on it.
fn shell() -> (HeadlessShell, WindowId) {
    let mut shell = HeadlessShell::new();
    let window = shell
        .create_window(&crcbl_shell::WindowDesc::default())
        .expect("headless always creates a window");
    (shell, window)
}

/// A request for `target` with no stage started yet, for a test that runs
/// [`Presenter::start_device`] itself.
fn request(
    target: crcbl_hal::SurfaceTarget,
    extent: (u32, u32),
    label: &str,
    pacing: Pacing,
    optional_features: Features,
    adapter_pin: Option<&str>,
) -> PendingPresenter {
    PendingPresenter::new(
        OpenStage::Done,
        target,
        extent,
        &PresenterDesc {
            label,
            adapter_pin,
            optional_features,
            pacing,
            ..PresenterDesc::default()
        },
    )
    .expect("the default ring is at least one frame deep")
}

/// Polls `pending` until it yields the presenter.
fn finish_open(mut pending: PendingPresenter) -> Presenter {
    loop {
        if let Some(presenter) = pending.poll().expect("the null backend cannot fail here") {
            return presenter;
        }
    }
}

// ---- pacing, without a device ----------------------------------------------

/// One 60 Hz cycle, for the observations below. The figures are arbitrary —
/// the resolution reads the arm, never the duration — but a plausible one
/// keeps the failure messages readable.
const OBSERVED_CYCLE: Duration = Duration::from_nanos(16_666_666);

/// The quantum a [`DisplayTiming::Stepped`] panel moves in — half the cycle
/// above, so the pair is the shape the seam's mapping actually produces: a
/// cycle that is a whole multiple of its step, and not equal to it, which
/// would have been reported as [`DisplayTiming::Fixed`] instead.
const OBSERVED_STEP: Duration = Duration::from_nanos(8_333_333);

/// Every [`DisplayTiming`] arm, for the exhaustive sweeps below.
///
/// Written out rather than derived, so that a fifth arm on the enum is a
/// compile-time decision here and not a case these tests quietly stop
/// covering.
const OBSERVATIONS: [DisplayTiming; 4] = [
    DisplayTiming::Unknown,
    DisplayTiming::Fixed {
        cycle: OBSERVED_CYCLE,
    },
    DisplayTiming::Variable {
        shortest: OBSERVED_CYCLE,
    },
    DisplayTiming::Stepped {
        cycle: OBSERVED_CYCLE,
        step: OBSERVED_STEP,
    },
];

/// Every (requested, observed) pair the resolution can be handed.
///
/// **This test is the only place three of the four `DisplayTiming` arms
/// execute anywhere in this repo.** Every driver reachable from here
/// reports `Unknown`, so `Fixed`, `Variable` and `Stepped` reach the
/// resolution here and nowhere else, on any machine — which is why the
/// policy is a method with no device in its signature.
#[test]
fn auto_is_the_only_pacing_the_display_can_change() {
    for observed in OBSERVATIONS {
        for requested in [Pacing::Vsync, Pacing::Adaptive, Pacing::Off] {
            assert_eq!(
                requested.resolve(observed),
                requested,
                "{requested:?} was asked for by name; {observed:?} must not overrule it"
            );
        }
        assert_ne!(
            Pacing::Auto.resolve(observed),
            Pacing::Auto,
            "{observed:?} left Auto unresolved, so the loop would be pacing on a request"
        );
    }

    // The policy itself, arm by arm, so a change to it has to be written
    // down here as well as in `resolve`.
    assert_eq!(
        Pacing::Auto.resolve(DisplayTiming::Unknown),
        Pacing::Vsync,
        "a display that would not say is the fallback case"
    );
    assert_eq!(
        Pacing::Auto.resolve(DisplayTiming::Fixed {
            cycle: OBSERVED_CYCLE
        }),
        Pacing::Vsync,
        "a fixed cycle is exactly what vsync is for"
    );
    assert_eq!(
        Pacing::Auto.resolve(DisplayTiming::Variable {
            shortest: OBSERVED_CYCLE
        }),
        Pacing::Adaptive,
        "a free-running display is the case adaptive sync exists for"
    );
    assert_eq!(
        Pacing::Auto.resolve(DisplayTiming::Stepped {
            cycle: OBSERVED_CYCLE,
            step: OBSERVED_STEP
        }),
        Pacing::Adaptive,
        "a quantised cycle is not a fixed one, so a fixed-vblank wait is wrong there too"
    );
}

/// Which of those resolutions costs a swapchain rebuild, decided from the
/// surface's own mode list — no device, no display, just the data a
/// `SurfaceCaps` carries.
///
/// The pairing is the whole point: `Auto` resolving to `Vsync` must leave
/// the swapchain alone, because that is the case every machine in CI takes
/// and a rebuild there would be a start-up cost paid by every run for
/// nothing.
#[test]
fn only_a_display_that_is_not_fixed_costs_a_rebuild() {
    use crcbl_hal::SurfaceCaps;

    let caps = |present_modes: Vec<PresentMode>| SurfaceCaps {
        formats: vec![Format::Bgra8UnormSrgb],
        present_modes,
        composite_alpha: vec![CompositeAlpha::Opaque],
        min_image_count: 2,
        max_image_count: 3,
        current_extent: None,
    };
    let vrr_capable = caps(vec![PresentMode::Fifo, PresentMode::FifoRelaxed]);
    let fifo_only = caps(vec![PresentMode::Fifo]);

    // What a presenter opened on `Auto` is presenting with when the
    // resolution runs: `Auto`'s preference list, resolved once by the
    // surface. Read from the same call the presenter uses rather than written
    // down, so the two cannot drift.
    let opened = vrr_capable.choose_present_mode(Pacing::Auto.preferences());
    assert_eq!(opened, PresentMode::Fifo);

    for observed in OBSERVATIONS {
        let effective = Pacing::Auto.resolve(observed);
        let wanted = vrr_capable.choose_present_mode(effective.preferences());
        let rebuilds = wanted != opened;
        assert_eq!(
            rebuilds,
            matches!(
                observed,
                DisplayTiming::Variable { .. } | DisplayTiming::Stepped { .. }
            ),
            "{observed:?} resolved to {effective:?} and wanted {wanted:?}, \
             which is the wrong side of a swapchain rebuild"
        );
    }

    // The same resolution on a surface that has nothing to rebuild *to*.
    // `choose_present_mode` falls back to Fifo, so the mode does not change
    // and no swapchain is spent finding that out.
    for observed in OBSERVATIONS {
        let effective = Pacing::Auto.resolve(observed);
        assert_eq!(
            fifo_only.choose_present_mode(effective.preferences()),
            fifo_only.choose_present_mode(Pacing::Auto.preferences()),
            "{observed:?} rebuilt a Fifo-only surface to the mode it already had"
        );
    }
}

/// Which present a frame waits for, without a GPU and without spending the
/// time.
#[test]
fn a_frame_waits_for_the_present_frames_in_flight_behind_it() {
    let depth = FRAMES_IN_FLIGHT as u64;

    // Still filling: the first `FRAMES_IN_FLIGHT` frames have nothing that
    // far back, and present ids start at 1 rather than 0.
    for submitted in 0..depth {
        assert_eq!(
            Presenter::present_to_wait_for(submitted, Pacing::Vsync, FRAMES_IN_FLIGHT),
            None,
            "frame {} has only {submitted} presents behind it",
            submitted + 1
        );
    }

    for submitted in depth..depth + 8 {
        let waited = Presenter::present_to_wait_for(submitted, Pacing::Vsync, FRAMES_IN_FLIGHT)
            .expect("the loop has filled");
        assert_eq!(
            waited,
            submitted + 1 - depth,
            "the frame about to start is {}, so it waits {depth} back",
            submitted + 1
        );
        assert!(
            waited < submitted,
            "waiting on present {submitted}, the one just submitted, would drain the \
             pipeline to a single frame"
        );
    }
}

/// `Pacing::Off` asked not to be paced by the display, and a wait for a
/// frame to be on screen is exactly that.
#[test]
fn pacing_off_waits_for_nothing() {
    for submitted in 0..8 {
        assert_eq!(
            Presenter::present_to_wait_for(submitted, Pacing::Off, FRAMES_IN_FLIGHT),
            None,
            "submitted {submitted}"
        );
        assert!(
            Presenter::present_to_wait_for(submitted, Pacing::Adaptive, FRAMES_IN_FLIGHT).is_some()
                == Presenter::present_to_wait_for(submitted, Pacing::Vsync, FRAMES_IN_FLIGHT)
                    .is_some(),
            "adaptive still follows the display when it can, so it waits when vsync does"
        );
    }
}

/// **A ring of no frames is refused when the open starts**, rather than
/// underflowing the retirement depth on the first submit.
#[test]
fn a_ring_of_no_frames_is_refused() {
    let (shell, window) = shell();
    let refused = Presenter::request_open_on(
        Box::new(crcbl_hal::null::NullInstance::gpu_driven()),
        &shell,
        window,
        (320, 240),
        &PresenterDesc {
            frames_in_flight: 0,
            ..PresenterDesc::default()
        },
    )
    .expect_err("a presenter cannot keep no frames in flight");
    assert!(
        matches!(
            refused,
            PresentError::Unusable("a presenter needs at least one frame in flight")
        ),
        "{refused}"
    );
}

// ---- opening -----------------------------------------------------------------

/// The browser's start-up shape, driven on a headless shell with the null
/// backend: `request_open` never blocks, and polling it produces exactly
/// the presenter `open` would have produced.
///
/// `open` itself is this loop with a `yield_now` in it, so a break in the
/// state machine breaks both — which is the point of having only one.
#[test]
fn a_presenter_can_be_opened_by_polling_instead_of_blocking() {
    let (shell, window) = shell();
    let mut pending = Presenter::request_open(
        &shell,
        window,
        (320, 240),
        &PresenterDesc {
            label: "polled open",
            backend: Some(GpuBackend::Null),
            ..PresenterDesc::default()
        },
    )
    .expect("the null backend is always registered");

    let mut polls = 0;
    let mut gpu = loop {
        polls += 1;
        assert!(polls < 64, "the null backend must not poll forever");
        if let Some(presenter) = pending.poll().expect("nothing here can fail") {
            break presenter;
        }
    };
    assert_eq!(gpu.extent(), (320, 240));
    assert_eq!(gpu.frames_in_flight(), FRAMES_IN_FLIGHT);
    assert_eq!(
        gpu.device().backend(),
        crcbl_hal::BackendKind::Null,
        "the backend asked for by value is the one that opened"
    );

    // Polling a spent request is a caller bug, not a second presenter.
    let error = pending.poll().expect_err("the presenter was already taken");
    assert!(matches!(error, PresentError::Unusable(_)), "{error}");

    gpu.drain().expect("nothing was submitted");
    gpu.destroy()
        .expect("teardown is in the seam's stated order");
}

/// The start of topic 39's downgrade line, exactly as
/// [`PendingPresenter::poll`] writes it.
const DOWNGRADE_LINE: &str = "hal: this device does not have";

/// Opens a presenter on a null instance the caller picked, which the registry
/// cannot do: [`crate::backend`]'s null entry is always the `gpu_driven`
/// preset, and a downgrade needs a device that grants less than that.
///
/// The shell comes back with the presenter because it owns the window the
/// surface was made for, and `create_surface`'s contract has that window
/// outliving the surface.
fn open_null_context(
    instance: crcbl_hal::null::NullInstance,
    optional_features: Features,
    pacing: Pacing,
) -> (HeadlessShell, Presenter) {
    let (shell, window) = shell();
    let pending = Presenter::request_open_on(
        Box::new(instance),
        &shell,
        window,
        (320, 240),
        &PresenterDesc {
            label: "downgrade test",
            optional_features,
            pacing,
            ..PresenterDesc::default()
        },
    )
    .expect("the null backend opens everywhere");
    let gpu = finish_open(pending);
    (shell, gpu)
}

/// **The downgrade line is an assertion target, not decoration.**
///
/// Topic 39 makes it the engine's only evidence that a
/// device refused an optional feature. Nothing but this test reads it, so
/// without it a refactor could delete the `log::info!` in
/// [`PendingPresenter::poll`] and leave the suite green.
#[test]
fn a_device_that_grants_less_says_so() {
    use crcbl_hal::null::NullInstance;

    let logs = crcbl_core::log::capture();
    let (_shell, mut gpu) = open_null_context(
        // The WebGPU-shaped floor, asked for the GPU-driven bundle every game
        // asks for by default: it has neither bindless nor a GPU-side draw
        // count.
        NullInstance::portable(),
        PresenterDesc::default().optional_features | Features::GPU_DRIVEN,
        Pacing::Off,
    );

    let records = logs.records();
    let said: Vec<_> = records
        .iter()
        .filter(|record| record.message.starts_with(DOWNGRADE_LINE))
        .collect();
    assert_eq!(said.len(), 1, "once, at device creation: {records:?}");
    assert_eq!(said[0].level, log::Level::Info);
    assert!(
        said[0].message.contains("DESCRIPTOR_INDEXING -> binding"),
        "the line names the feature and the selector its absence moved: {}",
        said[0].message
    );

    gpu.drain().expect("nothing was submitted");
    gpu.destroy()
        .expect("teardown is in the seam's stated order");
}

/// The silence is the other half of the claim and the easier half to lose: a
/// line logged unconditionally reads as "downgraded to nothing" on every
/// device that granted the lot, which is most of them.
#[test]
fn a_device_that_grants_everything_says_nothing() {
    use crcbl_hal::null::NullInstance;

    let logs = crcbl_core::log::capture();
    let (_shell, mut gpu) = open_null_context(
        NullInstance::gpu_driven(),
        // Every flag here is in that preset's caps. The presenter's own default
        // set is deliberately not used: it asks for `PRESENT_FEEDBACK` and
        // `PRESENT_TIMING`, which this preset does not have, and their absence
        // is a real downgrade.
        Features::GPU_DRIVEN | Features::TIMESTAMP_QUERY | Features::DEBUG_MARKERS,
        Pacing::Off,
    );

    let records = logs.records();
    assert!(
        records
            .iter()
            .any(|record| record.message.contains("null adapter")),
        "the capture is live — this is the adapter line every open logs, and \
         without it the assertion below would pass on an empty buffer: {records:?}"
    );
    assert!(
        !records
            .iter()
            .any(|record| record.message.starts_with(DOWNGRADE_LINE)),
        "nothing was lost, so nothing is said: {records:?}"
    );

    gpu.drain().expect("nothing was submitted");
    gpu.destroy()
        .expect("teardown is in the seam's stated order");
}

// ---- the adapter walk --------------------------------------------------------

/// The debug line `start_device` writes for each adapter it passes over.
const REFUSED_LINE: &str = "hal: adapter ";

/// A start-up request for a headless window, with `adapter_pin` standing
/// in for [`crate::adapter::pin`] so no test touches the process
/// environment.
///
/// The shell comes back with it because the request's surface target names
/// that shell's window.
fn adapter_walk_request(adapter_pin: Option<&str>) -> (HeadlessShell, PendingPresenter) {
    let (shell, window) = shell();
    let target = shell
        .surface_target(window)
        .expect("the headless window is still alive");
    let pending = request(
        target,
        (320, 240),
        "adapter walk test",
        Pacing::Off,
        Features::empty(),
        adapter_pin,
    );
    (shell, pending)
}

/// Runs [`Presenter::start_device`] against `instance` with `adapter_pin`
/// set, and hands back whatever it refused with.
///
/// The shell is dropped on the way out, which is only safe because nothing
/// survived the call: a failed start-up destroys the surface it made before
/// returning, so no object outlives the window it was made for.
fn start_device_error(
    instance: crcbl_hal::null::NullInstance,
    adapter_pin: Option<&str>,
) -> PresentError {
    let (_shell, pending) = adapter_walk_request(adapter_pin);
    match Presenter::start_device(Box::new(instance), &pending) {
        Ok(_) => panic!("nothing could serve this surface, so start-up must not have"),
        Err(error) => error.into(),
    }
}

/// **The adapter walk passes over one that cannot present and opens the
/// device on a later one.**
///
/// The case `crates/crcbl/tests/windowed_e2e.rs` says the loop exists for: a
/// discrete RADV GPU enumerates first under Xvfb and cannot present to the
/// window at all, while the software rasteriser behind it can. Until
/// [`Recorder::refuse_surface_on`](crcbl_hal::null::Recorder::refuse_surface_on)
/// and
/// [`NullInstance::with_adapters`](crcbl_hal::null::NullInstance::with_adapters)
/// existed, the null backend reported one adapter that never refused, so
/// `break`ing out of this loop on the first `Err` would have left the whole
/// suite green.
///
/// **The observable is `Presenter::adapter`** — the id `request_device` was
/// actually called with — not that start-up succeeded. A `break` on the
/// first refusal fails start-up outright, but a walk that took the *first*
/// adapter's caps and the *second* adapter's id, or the reverse, would
/// succeed here and be wrong on the machine this is modelling. The adapter
/// line the presenter logs is asserted beside it, because it is the only
/// record a run leaves of which adapter it took.
#[test]
fn the_adapter_walk_passes_over_one_that_cannot_present() {
    use crcbl_hal::AdapterId;
    use crcbl_hal::null::{NullInstance, Recorder};

    let logs = crcbl_core::log::capture();
    let recorder = Recorder::new();
    recorder.refuse_surface_on(AdapterId(0));
    let (_shell, mut gpu) = open_null_context(
        NullInstance::gpu_driven()
            .with_adapters(2)
            .with_recorder(recorder),
        Features::empty(),
        Pacing::Off,
    );
    assert_eq!(
        gpu.adapter,
        AdapterId(1),
        "the device was requested from the adapter that served the surface"
    );

    let records = logs.records();
    let chosen: Vec<_> = records
        .iter()
        .filter(|record| record.message.contains(", geometry "))
        .collect();
    assert_eq!(
        chosen.len(),
        1,
        "one adapter is taken, and named once: {records:?}"
    );
    assert!(
        chosen[0].message.contains("null adapter #1"),
        "the line names the adapter that served, not the one that refused: {}",
        chosen[0].message
    );
    let passed_over: Vec<_> = records
        .iter()
        .filter(|record| record.message.starts_with(REFUSED_LINE))
        .collect();
    assert_eq!(
        passed_over.len(),
        1,
        "the refusal is not silent — it is the only record of why the first \
         adapter was not used: {records:?}"
    );
    assert!(
        passed_over[0].message.contains("null adapter #0"),
        "{}",
        passed_over[0].message
    );

    gpu.drain().expect("nothing was submitted");
    gpu.destroy()
        .expect("teardown is in the seam's stated order");
}

/// **An exhausted walk and an empty adapter list fail differently**, and
/// each says which happened.
///
/// Two failures, two errors, and the distinction is the whole point:
/// "every adapter here refused this window" is a pairing problem a user can
/// act on — a second GPU, a display not wired to the card — while "no
/// adapter" is a machine with no GPU at all. An assertion that only checked
/// `is_err` would pass with the two collapsed into one message, and the
/// run's only diagnostic would then name the wrong cause half the time.
///
/// The refusal arm also carries the *adapter's own* error out rather than
/// replacing it with a sentence of the presenter's, which is what makes a
/// real backend's reason — `crcbl-vk`'s "no queue family on this adapter can
/// present to this surface" — reach the log at all.
#[test]
fn an_exhausted_adapter_walk_and_an_empty_one_fail_differently() {
    use crcbl_hal::null::{NullInstance, ObjectKind, Recorder};
    use crcbl_hal::{AdapterId, BackendKind};

    let logs = crcbl_core::log::capture();
    let recorder = Recorder::new();
    recorder.refuse_surface_on(AdapterId(0));
    recorder.refuse_surface_on(AdapterId(1));
    let exhausted = start_device_error(
        NullInstance::gpu_driven()
            .with_adapters(2)
            .with_recorder(recorder.clone()),
        None,
    );
    assert!(
        matches!(
            exhausted,
            PresentError::Hal(HalError::Unsupported {
                backend: BackendKind::Null,
                ..
            })
        ),
        "the adapter's own refusal is what comes out: {exhausted}"
    );
    assert_eq!(
        logs.records()
            .iter()
            .filter(|record| record.message.starts_with(REFUSED_LINE))
            .count(),
        2,
        "both adapters were asked; the walk ran out rather than stopping at the first"
    );
    assert_eq!(
        recorder.live_objects(ObjectKind::Surface),
        0,
        "the surface start-up made is destroyed on the way out, or every failed \
         open leaks one"
    );

    let empty = start_device_error(NullInstance::gpu_driven().with_adapters(0), None);
    assert!(
        matches!(empty, PresentError::Unusable("no adapter")),
        "a machine with no GPU is not a window nothing can present to: {empty}"
    );
    assert_ne!(
        exhausted.to_string(),
        empty.to_string(),
        "the two causes must be readable apart in a log, not only in a `match`"
    );
}

/// One enumerated adapter of class `device_type` at position `index`.
///
/// Built by hand because every null adapter is
/// [`DeviceType::Cpu`](crcbl_hal::DeviceType::Cpu) (see
/// [`NullInstance::with_adapters`](crcbl_hal::null::NullInstance::with_adapters)),
/// and a pin is only a choice on an enumeration with more than one class
/// in it.
fn fake_adapter(index: u32, device_type: crcbl_hal::DeviceType) -> crcbl_hal::AdapterInfo {
    crcbl_hal::AdapterInfo {
        id: crcbl_hal::AdapterId(index),
        name: format!("{device_type:?} #{index}"),
        vendor_id: 0x1002,
        device_id: 0x744c,
        device_type,
        driver: "test".to_owned(),
        backend: crcbl_hal::BackendKind::Null,
        luid: None,
    }
}

/// **A pin narrows the walk to the adapter it names; no pin walks every
/// adapter.**
///
/// The shape is the one `CRCBL_ADAPTER=cpu` met on the D3D12 desktop: the
/// GPU enumerates first and WARP is appended last, so a walk that ignored
/// the pin opened the GPU and said nothing about it. The unpinned half is
/// the other promise — every adapter, ranked rather than narrowed, because
/// that is what lets the surface-aware walk step past a GPU that cannot
/// present.
#[test]
fn a_pin_narrows_the_adapter_walk_to_the_class_it_names() {
    use crcbl_hal::{AdapterId, DeviceType};

    let adapters = [
        fake_adapter(0, DeviceType::Discrete),
        fake_adapter(1, DeviceType::Integrated),
        fake_adapter(2, DeviceType::Cpu),
    ];
    let ids = |pin| {
        crate::adapter::choose(&adapters, pin, None, PowerPreference::HighPerformance, None).map(
            |choice| {
                choice
                    .candidates()
                    .iter()
                    .map(|info| info.id)
                    .collect::<Vec<_>>()
            },
        )
    };

    assert_eq!(ids(Some("cpu")), Ok(vec![AdapterId(2)]));
    assert_eq!(ids(Some("integrated")), Ok(vec![AdapterId(1)]));
    assert_eq!(ids(Some("discrete")), Ok(vec![AdapterId(0)]));
    assert_eq!(
        ids(None),
        Ok(vec![AdapterId(0), AdapterId(1), AdapterId(2)]),
        "unpinned, the walk is every adapter, ranked for the preference"
    );
    let miss = ids(Some("virtual")).expect_err("there is no virtual adapter to name");
    assert!(miss.to_string().contains("no Virtual adapter"), "{miss}");
}

/// **A pinned open takes the adapter and says it was pinned; an unpinned
/// one says it walked.**
///
/// The adapter line is the only record a windowed run leaves of which
/// device drew it, and "which adapter" without "why that one" cannot tell a
/// pin that worked from a pin that was never read — the defect this line
/// exists to show.
#[test]
fn the_adapter_line_says_whether_the_adapter_was_pinned() {
    use crcbl_hal::null::NullInstance;

    for (pin, why) in [
        (Some("cpu"), "pinned by CRCBL_ADAPTER=cpu"),
        (None, "CRCBL_ADAPTER unset"),
    ] {
        let logs = crcbl_core::log::capture();
        let (_shell, mut pending) = adapter_walk_request(pin);
        pending.stage = Presenter::start_device(Box::new(NullInstance::gpu_driven()), &pending)
            .unwrap_or_else(|error| panic!("{pin:?}: the null adapter is a Cpu: {error}"));
        let mut gpu = finish_open(pending);

        let records = logs.records();
        let chosen: Vec<_> = records
            .iter()
            .filter(|record| record.message.contains(", geometry "))
            .collect();
        assert_eq!(chosen.len(), 1, "{pin:?}: {records:?}");
        assert!(
            chosen[0].message.contains(why),
            "{pin:?}: the line says why this adapter: {}",
            chosen[0].message
        );

        gpu.drain().expect("nothing was submitted");
        gpu.destroy()
            .expect("teardown is in the seam's stated order");
    }
}

/// **A pinned adapter that cannot present is refused, naming the pin,
/// rather than walked past.**
///
/// Unpinned, the same refusal is the adapter's own `HalError` and the walk
/// would have gone on to the next adapter (see
/// `the_adapter_walk_passes_over_one_that_cannot_present`). Pinned, going
/// on would open an adapter nobody named, so the error has to say which
/// pin chose the adapter that could not present — otherwise a run with
/// `CRCBL_ADAPTER` set reads as a broken window rather than a wrong pin.
#[test]
fn a_pinned_adapter_that_cannot_present_is_refused_naming_the_pin() {
    use crcbl_hal::AdapterId;
    use crcbl_hal::null::{NullInstance, ObjectKind, Recorder};

    let recorder = Recorder::new();
    recorder.refuse_surface_on(AdapterId(0));
    let unpinned = start_device_error(
        NullInstance::gpu_driven().with_recorder(recorder.clone()),
        None,
    );
    let refused = start_device_error(
        NullInstance::gpu_driven().with_recorder(recorder.clone()),
        Some("cpu"),
    );
    let PresentError::AdapterPin(why) = &refused else {
        panic!("the pin is what failed, not the backend: {refused}");
    };
    let why = why.to_string();
    assert!(why.contains("CRCBL_ADAPTER=cpu"), "{why}");
    assert!(why.contains("cannot present"), "{why}");
    // The adapter's own reason rides along: it is the only thing that says
    // *why* this adapter and this window do not pair.
    assert!(
        matches!(unpinned, PresentError::Hal(_)),
        "unpinned, the adapter's refusal is the error: {unpinned}"
    );
    assert!(why.contains(&unpinned.to_string()), "{why}\n{unpinned}");
    assert_eq!(
        recorder.live_objects(ObjectKind::Surface),
        0,
        "the surface is destroyed on the way out"
    );
}

/// **A pin that names no adapter here refuses before a surface exists.**
///
/// The null backend enumerates only [`crcbl_hal::DeviceType::Cpu`]
/// adapters, so `discrete` names nothing — the `CRCBL_ADAPTER=cpu` on a
/// runner with no WARP case, turned around. Refusing is
/// [`crate::adapter::select`]'s decision; this is that the presenter asks it.
#[test]
fn a_pin_that_names_no_adapter_here_refuses_the_open() {
    use crcbl_hal::null::NullInstance;

    let refused = start_device_error(NullInstance::gpu_driven(), Some("discrete"));
    let PresentError::AdapterPin(why) = &refused else {
        panic!("a pin that names nothing must not open anything: {refused}");
    };
    assert!(why.to_string().contains("no Discrete adapter"), "{refused}");
}

/// A [`crcbl_hal::null::NullInstance`] whose adapters report `classes`,
/// one each, in enumeration order.
///
/// The null backend calls every adapter a [`crcbl_hal::DeviceType::Cpu`],
/// and it is right to: it has one set of capabilities. A pin is only a
/// choice on an enumeration that mixes classes, so this relabels the
/// enumeration, answers `display` for the window's display adapter, records
/// each adapter whose caps are asked for in `probes`, refuses `unopenable`'s
/// as a driver that will not open it, and forwards everything else
/// untouched.
#[derive(Debug)]
struct ClassedInstance {
    inner: crcbl_hal::null::NullInstance,
    classes: Vec<crcbl_hal::DeviceType>,
    display: Option<crcbl_hal::AdapterId>,
    probes: std::sync::Arc<std::sync::Mutex<Vec<crcbl_hal::AdapterId>>>,
    unopenable: Option<crcbl_hal::AdapterId>,
}

impl ClassedInstance {
    /// `inner`, relabelled with `classes`, with nothing else injected.
    fn new(inner: crcbl_hal::null::NullInstance, classes: Vec<crcbl_hal::DeviceType>) -> Self {
        Self {
            inner,
            classes,
            display: None,
            probes: std::sync::Arc::default(),
            unopenable: None,
        }
    }
}

impl Instance for ClassedInstance {
    fn backend(&self) -> crcbl_hal::BackendKind {
        self.inner.backend()
    }

    fn adapters(&self) -> Vec<crcbl_hal::AdapterInfo> {
        self.inner
            .adapters()
            .into_iter()
            .zip(&self.classes)
            .map(|(info, &device_type)| crcbl_hal::AdapterInfo {
                device_type,
                ..info
            })
            .collect()
    }

    unsafe fn create_surface(
        &self,
        target: &crcbl_hal::SurfaceTarget,
    ) -> Result<SurfaceHandle, HalError> {
        // SAFETY: forwarded unchanged, so the caller's obligation for
        // `target` is the inner instance's.
        unsafe { self.inner.create_surface(target) }
    }

    fn destroy_surface(&self, surface: SurfaceHandle) {
        self.inner.destroy_surface(surface);
    }

    fn surface_caps(
        &self,
        surface: SurfaceHandle,
        adapter: crcbl_hal::AdapterId,
    ) -> Result<crcbl_hal::SurfaceCaps, HalError> {
        self.inner.surface_caps(surface, adapter)
    }

    fn display_adapter(
        &self,
        surface: SurfaceHandle,
    ) -> Result<Option<crcbl_hal::AdapterId>, HalError> {
        // The inner instance still checks the handle.
        self.inner.display_adapter(surface)?;
        Ok(self.display)
    }

    fn adapter_caps(
        &self,
        adapter: crcbl_hal::AdapterId,
    ) -> Result<crcbl_hal::DeviceCaps, HalError> {
        self.probes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(adapter);
        if self.unopenable == Some(adapter) {
            return Err(HalError::Backend("this driver will not open it".to_owned()));
        }
        self.inner.adapter_caps(adapter)
    }

    fn request_device(
        &self,
        desc: &DeviceDesc<'_>,
    ) -> Result<Box<dyn crcbl_hal::PendingDevice>, HalError> {
        self.inner.request_device(desc)
    }
}

/// **The presenter opens on the adapter the pin names, and on the first one
/// without a pin.**
///
/// The enumeration is D3D12's on this desktop: a GPU first, the software
/// rasteriser after it. Before the engine asked [`crate::adapter::select`],
/// `CRCBL_ADAPTER=cpu` opened the GPU here and the harness printed the
/// mismatch. The adapter is read back from the finished presenter rather
/// than from the walk, because the device is what the pin is about.
#[test]
fn the_context_opens_on_the_adapter_the_pin_names() {
    use crcbl_hal::null::NullInstance;
    use crcbl_hal::{AdapterId, DeviceType};

    for (pin, want) in [
        (Some("cpu"), (AdapterId(1), DeviceType::Cpu)),
        (None, (AdapterId(0), DeviceType::Discrete)),
    ] {
        let (_shell, mut pending) = adapter_walk_request(pin);
        let instance = ClassedInstance::new(
            NullInstance::gpu_driven().with_adapters(2),
            vec![DeviceType::Discrete, DeviceType::Cpu],
        );
        pending.stage = Presenter::start_device(Box::new(instance), &pending)
            .unwrap_or_else(|error| panic!("{pin:?}: both adapters can present: {error}"));
        let mut gpu = finish_open(pending);

        let opened = gpu
            .adapter()
            .expect("the presenter's own adapter is enumerable");
        assert_eq!(
            (opened.id, opened.device_type),
            want,
            "{pin:?}: the presenter opened {:?}",
            opened.name
        );

        gpu.drain().expect("nothing was submitted");
        gpu.destroy()
            .expect("teardown is in the seam's stated order");
    }
}

/// Starts a presenter on a desktop's enumeration — a discrete GPU, then an
/// integrated one — with `desc`'s adapter fields, and hands back the adapter
/// it opened on or what it refused with.
fn open_on_desktop(
    desc: &PresenterDesc<'_>,
    refuse: Option<crcbl_hal::AdapterId>,
) -> Result<crcbl_hal::AdapterId, PresentError> {
    open_on(desc, refuse, None)
}

/// [`open_on_desktop`], with the backend saying `display` drives the
/// window's display.
fn open_on(
    desc: &PresenterDesc<'_>,
    refuse: Option<crcbl_hal::AdapterId>,
    display: Option<crcbl_hal::AdapterId>,
) -> Result<crcbl_hal::AdapterId, PresentError> {
    use crcbl_hal::DeviceType;
    use crcbl_hal::null::{NullInstance, Recorder};

    let (shell, window) = shell();
    let target = shell
        .surface_target(window)
        .expect("the headless window is still alive");
    let mut pending = PendingPresenter::new(OpenStage::Done, target, (320, 240), desc)
        .expect("the default ring is at least one frame deep");
    let recorder = Recorder::new();
    if let Some(adapter) = refuse {
        recorder.refuse_surface_on(adapter);
    }
    let instance = ClassedInstance {
        display,
        ..ClassedInstance::new(
            NullInstance::gpu_driven()
                .with_adapters(2)
                .with_recorder(recorder),
            vec![DeviceType::Discrete, DeviceType::Integrated],
        )
    };
    pending.stage = Presenter::start_device(Box::new(instance), &pending)?;
    let mut gpu = finish_open(pending);
    let opened = gpu.adapter().expect("the presenter's own adapter").id;
    gpu.drain().expect("nothing was submitted");
    gpu.destroy()
        .expect("teardown is in the seam's stated order");
    Ok(opened)
}

/// **The power preference reaches the walk**: an app's presenter opens the
/// integrated GPU by default, and one asking for `HighPerformance` the
/// discrete one, on the same enumeration with no display to say otherwise.
#[test]
fn the_presenter_ranks_adapters_by_its_power_preference() {
    use crcbl_hal::AdapterId;

    assert_eq!(
        open_on_desktop(&PresenterDesc::default(), None).ok(),
        Some(AdapterId(1)),
        "a presenter defaults to LowPower"
    );
    let fast = PresenterDesc {
        power_preference: PowerPreference::HighPerformance,
        ..PresenterDesc::default()
    };
    assert_eq!(open_on_desktop(&fast, None).ok(), Some(AdapterId(0)));
    // Ranked, so an adapter that cannot present is still stepped past.
    assert_eq!(
        open_on_desktop(&fast, Some(AdapterId(0))).ok(),
        Some(AdapterId(1))
    );
}

/// **The display's adapter reaches the walk**: a window on a display the
/// discrete GPU drives opens it even though the presenter asks for
/// `LowPower`, and one the integrated GPU drives stays on it.
#[test]
fn the_presenter_asks_which_adapter_drives_the_display() {
    use crcbl_hal::AdapterId;

    let app = PresenterDesc::default();
    assert_eq!(
        open_on(&app, None, Some(AdapterId(0))).ok(),
        Some(AdapterId(0))
    );
    assert_eq!(
        open_on(&app, None, Some(AdapterId(1))).ok(),
        Some(AdapterId(1))
    );
}

/// **Adapters are probed in rank order, and the walk stops at the first
/// that serves**: listing them probes none, an adapter the driver will not
/// open is stepped past, and the adapters ranked after the one that opened
/// are never probed. On D3D12 each probe is a driver device, so a walk that
/// probed every adapter would be the start-up cost this exists to avoid.
#[test]
fn adapters_are_probed_in_rank_order_and_only_until_one_serves() {
    use crcbl_hal::null::NullInstance;
    use crcbl_hal::{AdapterId, DeviceType};

    let (shell, window) = shell();
    let target = shell
        .surface_target(window)
        .expect("the headless window is still alive");
    let mut pending = PendingPresenter::new(
        OpenStage::Done,
        target,
        (320, 240),
        &PresenterDesc::default(),
    )
    .expect("the default ring is at least one frame deep");
    let instance = ClassedInstance {
        unopenable: Some(AdapterId(1)),
        ..ClassedInstance::new(
            NullInstance::gpu_driven().with_adapters(3),
            vec![
                DeviceType::Discrete,
                DeviceType::Integrated,
                DeviceType::Cpu,
            ],
        )
    };
    let probes = std::sync::Arc::clone(&instance.probes);
    instance.adapters();
    assert!(
        probes.lock().expect("not poisoned").is_empty(),
        "listing adapters probes none"
    );

    pending.stage = Presenter::start_device(Box::new(instance), &pending)
        .unwrap_or_else(|error| panic!("the discrete adapter opens: {error}"));
    let mut gpu = finish_open(pending);
    assert_eq!(gpu.adapter().map(|info| info.id), Some(AdapterId(0)));
    // LowPower ranks the integrated adapter first; it will not open, so the
    // discrete one is next, and the software one is never asked.
    assert_eq!(
        *probes.lock().expect("not poisoned"),
        [AdapterId(1), AdapterId(0)]
    );
    gpu.drain().expect("nothing was submitted");
    gpu.destroy()
        .expect("teardown is in the seam's stated order");
}

/// **A program's pick opens the adapter it names and nothing else**: a
/// picked adapter that cannot present is refused naming the pick, not walked
/// past, and the variable's pin still wins over a pick.
#[test]
fn a_program_pick_opens_its_adapter_or_refuses() {
    use crcbl_hal::{AdapterId, DeviceType};

    let pick = PresenterDesc {
        adapter_pick: Some(AdapterPick::Class(DeviceType::Discrete)),
        ..PresenterDesc::default()
    };
    assert_eq!(open_on_desktop(&pick, None).ok(), Some(AdapterId(0)));

    let refused = open_on_desktop(&pick, Some(AdapterId(0))).expect_err("only the pick is tried");
    let PresentError::AdapterPin(why) = &refused else {
        panic!("the pick is what failed, not the backend: {refused}");
    };
    let why = why.to_string();
    assert!(why.contains("the program's pick"), "{why}");
    assert!(why.contains("cannot present"), "{why}");

    let pinned = PresenterDesc {
        adapter_pin: Some("integrated"),
        ..pick
    };
    assert_eq!(open_on_desktop(&pinned, None).ok(), Some(AdapterId(1)));
}

// ---- composite alpha ---------------------------------------------------------

/// **The composite alpha a presenter asks for, over every set of modes a
/// surface could offer.**
///
/// Red when a transparent window falls back to anything (opaque, straight
/// alpha, inherit) instead of being refused, when an opaque window starts
/// taking premultiplied because it is offered, or when an opaque window
/// is refused for a surface that offers no premultiplied alpha.
#[test]
fn a_transparent_window_asks_for_premultiplied_or_nothing() {
    const MODES: [CompositeAlpha; 4] = [
        CompositeAlpha::Opaque,
        CompositeAlpha::PreMultiplied,
        CompositeAlpha::PostMultiplied,
        CompositeAlpha::Inherit,
    ];
    let mut checked = 0;
    for subset in 0..1_u32 << MODES.len() {
        let offered: Vec<_> = MODES
            .iter()
            .enumerate()
            .filter(|(bit, _)| subset & (1 << bit) != 0)
            .map(|(_, &mode)| mode)
            .collect();
        assert_eq!(
            composite_alpha_for(false, &offered),
            Some(CompositeAlpha::Opaque),
            "an opaque window is opaque whatever is offered: {offered:?}"
        );
        let premultiplied = offered.contains(&CompositeAlpha::PreMultiplied);
        assert_eq!(
            composite_alpha_for(true, &offered),
            premultiplied.then_some(CompositeAlpha::PreMultiplied),
            "a transparent window is premultiplied or refused: {offered:?}"
        );
        checked += 1;
    }
    assert_eq!(checked, 1 << MODES.len(), "every subset was checked");
}

/// `inner`, with its surface offering, on adapter `n`, the composite-alpha
/// modes `offers[n]`.
///
/// The null backend offers [`CompositeAlpha::Opaque`] alone, as a surface
/// with no compositor behind it should; a transparent window needs a
/// surface that offers more. The offer goes through the backend's own
/// recorder, so its swapchains are held to the same list its caps report.
fn alpha_instance(
    inner: crcbl_hal::null::NullInstance,
    offers: &[Vec<CompositeAlpha>],
) -> crcbl_hal::null::NullInstance {
    let recorder = inner.recorder();
    for (adapter, modes) in (0..).zip(offers) {
        recorder.offer_composite_alpha_on(crcbl_hal::AdapterId(adapter), modes);
    }
    inner
}

/// A Wayland target for a window that was or was not made transparent.
///
/// The null backend's `create_surface` reads nothing but the variant's
/// name, so the dangling handles are never dereferenced; a headless
/// window's target is `Offscreen`, which carries no transparency at all.
fn wayland_target(transparent: bool) -> crcbl_hal::SurfaceTarget {
    crcbl_hal::SurfaceTarget::Wayland {
        display: core::ptr::NonNull::dangling(),
        surface: core::ptr::NonNull::dangling(),
        transparent,
        main_device: None,
    }
}

/// Starts and finishes an open of `target` against `instance`, as
/// [`open_null_context`] does for a headless window.
fn open_alpha_context(
    instance: crcbl_hal::null::NullInstance,
    target: crcbl_hal::SurfaceTarget,
    adapter_pin: Option<&str>,
) -> Result<(HeadlessShell, Presenter), PresentError> {
    open_alpha_context_with(instance, target, adapter_pin, Transparency::Required)
}

/// [`open_alpha_context`], with what a transparent window may fall back to.
fn open_alpha_context_with(
    instance: crcbl_hal::null::NullInstance,
    target: crcbl_hal::SurfaceTarget,
    adapter_pin: Option<&str>,
    transparency: Transparency,
) -> Result<(HeadlessShell, Presenter), PresentError> {
    let (shell, mut pending) = adapter_walk_request(adapter_pin);
    pending.target = target;
    pending.transparency = transparency;
    pending.stage = Presenter::start_device(Box::new(instance), &pending)?;
    let gpu = loop {
        if let Some(presenter) = pending.poll()? {
            break presenter;
        }
    };
    Ok((shell, gpu))
}

/// **A transparent window opens premultiplied and keeps it through a
/// resize and a pacing change; an opaque one on the same surface stays
/// opaque.**
///
/// Read from the presenter's `SwapchainConfig`, which is what every
/// rebuild's descriptor is made from, and from that descriptor itself.
/// Red when `SwapchainConfig::desc` goes back to a fixed
/// `CompositeAlpha::Opaque`, when the open ignores the target's flag, or
/// when a reconfigure path rebuilds the config without the alpha.
#[test]
fn a_transparent_window_presents_premultiplied_across_reconfigures() {
    use crcbl_hal::null::NullInstance;

    let both = || {
        alpha_instance(
            NullInstance::gpu_driven(),
            &[vec![CompositeAlpha::Opaque, CompositeAlpha::PreMultiplied]],
        )
    };
    let (_shell, mut gpu) =
        open_alpha_context(both(), wayland_target(true), None).expect("premultiplied is offered");
    assert_eq!(gpu.config.composite_alpha, CompositeAlpha::PreMultiplied);

    gpu.resize((640, 480)).expect("a resize");
    assert_eq!(gpu.config.extent, (640, 480), "the resize took");
    let before = gpu.config.present_mode;
    gpu.set_pacing(Pacing::Vsync).expect("a pacing change");
    assert_ne!(
        gpu.config.present_mode, before,
        "the pacing change rebuilt the swapchain with a new mode"
    );
    assert_eq!(
        gpu.config.composite_alpha,
        CompositeAlpha::PreMultiplied,
        "a rebuild keeps the alpha the presenter opened with"
    );
    assert_eq!(
        gpu.config.desc(gpu.surface).composite_alpha,
        CompositeAlpha::PreMultiplied,
        "the descriptor every rebuild is made from carries it"
    );
    gpu.drain().expect("nothing was submitted");
    gpu.destroy().expect("teardown");

    let (_shell, mut gpu) =
        open_alpha_context(both(), wayland_target(false), None).expect("opaque is offered");
    assert_eq!(
        gpu.config.desc(gpu.surface).composite_alpha,
        CompositeAlpha::Opaque,
        "an opaque window is not made premultiplied because it could be"
    );
    gpu.drain().expect("nothing was submitted");
    gpu.destroy().expect("teardown");
}

/// **A transparent window walks past an adapter whose surface cannot
/// composite it, and is refused by name when none can.**
///
/// Red when an adapter without premultiplied alpha is taken anyway (the
/// window drawn opaque), when the walk stops at it instead of going on,
/// and when the refusal stops naming the missing alpha, pinned or not.
#[test]
fn a_transparent_window_no_adapter_can_composite_is_refused_by_name() {
    use crcbl_hal::AdapterId;
    use crcbl_hal::null::{NullInstance, ObjectKind, Recorder};

    let opaque = vec![CompositeAlpha::Opaque];
    let both = vec![CompositeAlpha::Opaque, CompositeAlpha::PreMultiplied];
    let (_shell, mut gpu) = open_alpha_context(
        alpha_instance(
            NullInstance::gpu_driven().with_adapters(2),
            &[opaque.clone(), both],
        ),
        wayland_target(true),
        None,
    )
    .expect("the second adapter can composite the window");
    assert_eq!(gpu.adapter, AdapterId(1));
    assert_eq!(gpu.config.composite_alpha, CompositeAlpha::PreMultiplied);
    gpu.drain().expect("nothing was submitted");
    gpu.destroy().expect("teardown");

    let recorder = Recorder::new();
    let Err(refused) = open_alpha_context(
        alpha_instance(
            NullInstance::gpu_driven()
                .with_adapters(2)
                .with_recorder(recorder.clone()),
            &[opaque.clone(), opaque.clone()],
        ),
        wayland_target(true),
        None,
    ) else {
        panic!("no adapter can composite a transparent window, so none may open");
    };
    assert!(
        matches!(refused, PresentError::Unusable(NO_PREMULTIPLIED_ALPHA)),
        "{refused}"
    );
    assert_eq!(
        recorder.live_objects(ObjectKind::Surface),
        0,
        "the surface is destroyed on the way out"
    );

    let Err(pinned) = open_alpha_context(
        alpha_instance(NullInstance::gpu_driven(), &[opaque]),
        wayland_target(true),
        Some("cpu"),
    ) else {
        panic!("the pinned adapter cannot composite a transparent window");
    };
    let PresentError::AdapterPin(why) = &pinned else {
        panic!("the pin chose the adapter that could not: {pinned}");
    };
    assert!(why.to_string().contains(NO_PREMULTIPLIED_ALPHA), "{why}");
}

// ---- start-up lines ----------------------------------------------------------

/// The two halves of the present-feedback line [`Presenter::finish`] logs,
/// exactly as it writes them.
const FEEDBACK_LINE: &str = "hal: pacing on presents";
const NO_FEEDBACK_LINE: &str = "hal: no present feedback";

/// **Start-up says which of the two pacing stories this run gets.**
///
/// `docs/backlog.md` records that a richer `wait_until_presented` return —
/// one distinguishing "waited" from "this device cannot observe presents" —
/// was declined *because* `caps().features` answers it once here, which is
/// what lets `acquire` call the wait with no branch on which backend is
/// underneath. Delete this line and the argument for that decision has no
/// evidence left.
///
/// No `NullInstance` preset advertises [`Features::PRESENT_FEEDBACK`] —
/// there is no display under that backend, so `wait_until_presented` returns
/// at once and the preset is honest about it — so the adapter is built by
/// hand. Nothing here waits on a present: the line reads the device's caps
/// and that is the whole of what this exercises.
#[test]
fn a_device_that_can_observe_presents_says_so() {
    use crcbl_hal::null::NullInstance;
    use crcbl_hal::{DeviceCaps, Limits};

    let logs = crcbl_core::log::capture();
    let (_shell, mut gpu) = open_null_context(
        NullInstance::new(DeviceCaps {
            features: Features::PRESENT_FEEDBACK,
            limits: Limits::minimum(),
        }),
        Features::PRESENT_FEEDBACK,
        Pacing::Off,
    );

    let records = logs.records();
    let said: Vec<_> = records
        .iter()
        .filter(|record| record.message.starts_with(FEEDBACK_LINE))
        .collect();
    assert_eq!(said.len(), 1, "once, at start-up: {records:?}");
    assert_eq!(said[0].level, log::Level::Info);
    assert_eq!(
        said[0].message,
        format!("hal: pacing on presents, {FRAMES_IN_FLIGHT} frames deep"),
        "the line names the depth the loop actually runs at",
    );
    assert!(
        !records
            .iter()
            .any(|record| record.message.starts_with(NO_FEEDBACK_LINE)),
        "the other story is not this run's: {records:?}"
    );

    gpu.drain().expect("nothing was submitted");
    gpu.destroy()
        .expect("teardown is in the seam's stated order");
}

/// The other state, and the one every device in this repo is actually in:
/// the presenter asked for present feedback and the device did not have it,
/// so the frame limiter is all that paces the loop.
#[test]
fn a_device_that_cannot_observe_presents_says_that_instead() {
    use crcbl_hal::null::NullInstance;

    let logs = crcbl_core::log::capture();
    let (_shell, mut gpu) = open_null_context(
        NullInstance::gpu_driven(),
        // The presenter's default set, which asks for `PRESENT_FEEDBACK`; this
        // preset does not have it.
        PresenterDesc::default().optional_features,
        Pacing::Off,
    );

    let records = logs.records();
    let said: Vec<_> = records
        .iter()
        .filter(|record| record.message.starts_with(NO_FEEDBACK_LINE))
        .collect();
    assert_eq!(said.len(), 1, "once, at start-up: {records:?}");
    assert_eq!(said[0].level, log::Level::Debug);
    assert_eq!(
        said[0].message,
        "hal: no present feedback; the frame limiter is the only pacing",
    );
    assert!(
        !records
            .iter()
            .any(|record| record.message.starts_with(FEEDBACK_LINE)),
        "a line logged on both paths would report every device as pacing on \
         presents: {records:?}"
    );

    gpu.drain().expect("nothing was submitted");
    gpu.destroy()
        .expect("teardown is in the seam's stated order");
}

/// The prefix `crates/crcbl-shell/tests/run-wayland-e2e.sh` greps for, and
/// the start of the line `Presenter::settle_pacing` writes.
const PACING_LINE: &str = "hal: display timing ";

/// **The presenter says which pair it resolved, not just what it landed on.**
///
/// `Pacing::resolve`'s own test walks every (request, observation) pair;
/// what nothing read until now is that a *run* reports the pair it took.
/// "Asked for `Auto`, display said nothing, running vsync" and "asked for
/// `Off`" are different runs, and a line naming only the result cannot tell
/// them apart — so two requests are driven through here and the whole line
/// asserted for each.
///
/// The observation half is `Unknown` in both, because the null device has no
/// display to have a cadence and says so; the other three
/// [`DisplayTiming`] arms need a driver that has never answered anything
/// else on any machine this repo has run on (see
/// [`Presenter::effective_pacing`]).
#[test]
fn the_pacing_resolution_says_which_pair_it_took() {
    use crcbl_hal::CommandEncoderDesc;
    use crcbl_hal::null::NullInstance;

    let logs = crcbl_core::log::capture();
    // A present, because the display is only asked after one — see
    // `settle_pacing` for why that order is the only one that can work.
    for pacing in [Pacing::Auto, Pacing::Off] {
        let (_shell, mut gpu) =
            open_null_context(NullInstance::gpu_driven(), Features::empty(), pacing);
        let acquired = gpu.acquire().expect("acquire").expect("no resize happened");
        let encoder = gpu.device().create_command_encoder(&CommandEncoderDesc {
            label: Some("pacing test"),
            queue: gpu.queue(),
        });
        let command_buffer = encoder.finish().expect("an empty command buffer");
        assert_eq!(
            gpu.submit_and_present(&acquired, command_buffer)
                .expect("present"),
            FrameOutcome::Presented,
        );
        gpu.drain().expect("the frame was submitted and presented");
        gpu.destroy()
            .expect("teardown is in the seam's stated order");
    }

    let records = logs.records();
    let said: Vec<_> = records
        .iter()
        .filter(|record| record.message.starts_with(PACING_LINE))
        .collect();
    assert_eq!(said.len(), 2, "once per presenter: {records:?}");
    assert!(said.iter().all(|record| record.level == log::Level::Info));
    assert_eq!(
        said[0].message, "hal: display timing Unknown; asked for Auto, pacing Vsync",
        "a display that will not say is the vsync fallback, and `Auto` is \
         not the answer it resolved to",
    );
    assert_eq!(
        said[1].message, "hal: display timing Unknown; asked for Off, pacing Off",
        "a concrete request comes back unchanged, and the line still says so",
    );
}

// ---- frames ------------------------------------------------------------------

/// The whole join, against the null backend: shell → surface → device →
/// swapchain → acquire → submit → present → teardown, through the blocking
/// [`Presenter::open`].
#[test]
fn the_join_runs_end_to_end_on_the_null_backend() {
    use crcbl_hal::CommandEncoderDesc;

    let (mut shell, window) = shell();
    let mut events = 0;
    let extent = wait_for_configure(&mut shell, window, &mut |_| events += 1).expect("configured");

    let mut gpu = Presenter::open(
        &shell,
        window,
        extent,
        &PresenterDesc {
            label: "presenter test",
            backend: Some(GpuBackend::Null),
            ..PresenterDesc::default()
        },
    )
    .expect("the null backend opens everywhere");
    assert_eq!(gpu.extent(), extent);

    for _ in 0..4 {
        let Some(acquired) = gpu.acquire().expect("acquire") else {
            continue;
        };
        let encoder = gpu.device().create_command_encoder(&CommandEncoderDesc {
            label: Some("presenter test"),
            queue: gpu.queue(),
        });
        let command_buffer = encoder.finish().expect("an empty command buffer");
        assert_eq!(
            gpu.submit_and_present(&acquired, command_buffer)
                .expect("present"),
            FrameOutcome::Presented,
        );
    }

    gpu.resize((640, 480)).expect("resize");
    assert_eq!(
        gpu.acquire().expect("acquire").map(|f| f.extent),
        Some((640, 480))
    );
    gpu.destroy().expect("teardown");
    shell.destroy_window(window).expect("the window goes away");
}

/// **An offscreen presenter acquires, presents and tears down like a
/// windowed one**, with no shell at all.
#[test]
fn an_offscreen_presenter_presents_with_no_window() {
    use crcbl_hal::CommandEncoderDesc;

    let mut gpu = Presenter::open_offscreen(
        (64, 32),
        &PresenterDesc {
            label: "offscreen presenter test",
            backend: Some(GpuBackend::Null),
            ..PresenterDesc::default()
        },
    )
    .expect("the null backend opens everywhere");
    let acquired = gpu
        .acquire()
        .expect("acquire")
        .expect("an offscreen ring never goes out of date");
    assert_eq!(acquired.extent, (64, 32));
    let encoder = gpu.device().create_command_encoder(&CommandEncoderDesc {
        label: Some("offscreen presenter test"),
        queue: gpu.queue(),
    });
    let command_buffer = encoder.finish().expect("an empty command buffer");
    assert_eq!(
        gpu.submit_and_present(&acquired, command_buffer)
            .expect("present"),
        FrameOutcome::Presented
    );
    gpu.destroy().expect("teardown");
}

/// **When the compositor picks a different size, the presenter adopts it —
/// and stops asking once it has.**
///
/// "Obligation 3: the answer, not the request". A window system may hand
/// back a swapchain smaller than the one asked for, and a caller that keeps
/// rendering at its requested extent draws off the end of the image. The
/// branch that writes the answer back into `configured_extent` and `config`
/// existed and was reachable from no test in this workspace and no device
/// it can run headlessly: the null backend had no window system to disagree
/// with, so the acquired extent was the configured one by construction.
/// `Recorder::clamp_acquired_extent_to` is what makes it disagree.
///
/// Both halves matter. `config.extent` has to move too, because `resize`
/// early-returns when the requested extent already matches it — leaving it
/// holding the size the shell asked for made a resize back to the
/// compositor's own choice look like a change and reconfigure for nothing.
#[test]
fn a_compositors_own_extent_is_adopted_and_then_agreed_with() {
    use crcbl_hal::null::{NullInstance, Recorder};

    let clamped = (256, 200);
    let recorder = Recorder::new();
    recorder.clamp_acquired_extent_to(clamped);
    let (_shell, mut gpu) = open_null_context(
        NullInstance::gpu_driven().with_recorder(recorder),
        Features::empty(),
        Pacing::Off,
    );
    assert_ne!(
        gpu.configured_extent, clamped,
        "the fixture must start at a different size, or this checks nothing"
    );

    let acquired = gpu
        .acquire()
        .expect("the null swapchain acquires")
        .expect("a frame, not a skipped one");
    assert_eq!(
        acquired.extent, clamped,
        "the injector is what makes the two disagree"
    );
    // The images the frame will draw into are that size too: the injector
    // clamps where a platform does, when the ring is built, and
    // `build_ring` creates the images and records the swapchain's extent
    // from one variable. Not asserted here because the null backend keeps
    // no image detail to read it back from — it is structural, and stated
    // where it is made rather than checked where it is used.
    assert_eq!(
        gpu.configured_extent, clamped,
        "the presenter renders at the answer, not the request"
    );
    assert_eq!(
        gpu.config.extent, clamped,
        "and the config holds it too, or a resize back to this size looks like a change"
    );

    // Having adopted it, the sizes agree and the branch stops firing. A
    // standing clamp is what lets this be observed: the second acquire
    // reports the same extent and must change nothing.
    let logs = crcbl_core::log::capture();
    gpu.acquire()
        .expect("the second acquire succeeds")
        .expect("a frame");
    assert_eq!(
        (gpu.configured_extent, gpu.config.extent),
        (clamped, clamped)
    );
    assert!(
        !logs
            .records()
            .iter()
            .any(|record| record.message.contains("swapchain configured at")),
        "the sizes already agree, so nothing is re-adopted: {:?}",
        logs.records()
    );
}

/// **A wait that finishes unsatisfied does not retire the buffer it was
/// waiting on.**
///
/// `wait_semaphores` answers `Ok(false)` for a wait it did not satisfy, and
/// `retire_to` used to discard that `bool` and destroy the command buffer on
/// the next line — freeing memory the device may still be reading. `u64::MAX`
/// did not make it unreachable: the null device answers from its recorded
/// timeline and never looks at `timeout_ns`, which is exactly what this
/// fixture leans on.
///
/// The queue is loaded directly rather than by rendering, because a frame
/// this presenter submits signals the value it then waits for — there is no
/// sequence of public calls that leaves a value outstanding, which is why
/// the arm went unexercised.
#[test]
fn an_unsatisfied_wait_leaves_the_command_buffer_in_flight() {
    use crcbl_hal::null::NullInstance;

    let (_shell, mut gpu) = open_null_context(
        NullInstance::gpu_driven(),
        Features::TIMELINE_SEMAPHORE,
        Pacing::Off,
    );
    let semaphore = gpu.timeline.expect("the preset carries a timeline");

    // A value nothing has signalled. The device's timeline sits below it,
    // so the wait completes and reports that it was not satisfied.
    let command_buffer = gpu
        .device
        .create_command_encoder(&crcbl_hal::CommandEncoderDesc {
            label: Some("never submitted"),
            queue: gpu.queue,
        })
        .finish()
        .expect("an empty encoder finishes");
    let outstanding = u64::MAX / 2;
    gpu.in_flight.push_back((outstanding, command_buffer));

    let error = gpu
        .retire_to(0)
        .expect_err("the wait cannot be satisfied, so the buffer must not be retired");
    assert!(
        matches!(error, PresentError::Unusable(_)),
        "an unsatisfied wait is not a HAL failure — the call succeeded and said no: {error:?}"
    );
    assert_eq!(
        gpu.in_flight.front().map(|(value, _)| *value),
        Some(outstanding),
        "the buffer stays queued, still owned: dropping it here would leak it instead of \
         freeing it once the timeline catches up"
    );

    // And the same call retires it once the timeline has reached the value,
    // so the refusal is about *this* wait and not about the queue being
    // non-empty.
    gpu.device
        .signal_semaphore(semaphore, outstanding)
        .expect("the host can advance a timeline");
    gpu.retire_to(0)
        .expect("a satisfied wait retires the buffer");
    assert!(gpu.in_flight.is_empty(), "nothing is left in flight");
}

/// The wiring, read off the device rather than off the presenter: every
/// present is numbered, and every frame past the first `FRAMES_IN_FLIGHT`
/// waits for one that has already been presented.
#[test]
fn the_loop_numbers_its_presents_and_waits_for_the_older_ones() {
    use crcbl_hal::null::Event;

    let (mut shell, window, recorder, mut gpu) = null_context("present pacing test", Pacing::Vsync);

    const FRAMES: u64 = 5;
    for _ in 0..FRAMES {
        assert_eq!(
            null_frame(&mut gpu).expect("present"),
            FrameOutcome::Presented,
        );
    }

    let events = recorder.events();
    let presented: Vec<Option<u64>> = events
        .iter()
        .filter_map(|event| match event {
            Event::Presented { present_id, .. } => Some(*present_id),
            _ => None,
        })
        .collect();
    assert_eq!(
        presented,
        (1..=FRAMES).map(Some).collect::<Vec<_>>(),
        "every present is numbered, from one, with no gaps"
    );

    let waited: Vec<u64> = events
        .iter()
        .filter_map(|event| match event {
            Event::PresentWaited { present_id, .. } => Some(*present_id),
            _ => None,
        })
        .collect();
    let depth = FRAMES_IN_FLIGHT as u64;
    assert_eq!(
        waited,
        (1..=FRAMES - depth).collect::<Vec<_>>(),
        "the first {depth} frames wait for nothing and the rest wait {depth} back"
    );

    // The same claim again, from the order of the stream rather than from
    // the counts: a wait never names a present that has not happened.
    let mut presented_so_far = 0;
    for event in &events {
        match event {
            Event::Presented { .. } => presented_so_far += 1,
            Event::PresentWaited { present_id, .. } => assert!(
                *present_id <= presented_so_far,
                "waited for present {present_id} with only {presented_so_far} presented"
            ),
            _ => {}
        }
    }

    gpu.destroy().expect("teardown");
    shell.destroy_window(window).expect("the window goes away");
}

/// **A frame is recorded while only the one before it may still run**:
/// after every submit, the frame just submitted is the only one left in
/// flight, so the next frame's writes into the `FRAMES_IN_FLIGHT`-deep
/// ring land in a slot the GPU has finished with. Under `Pacing::Off`,
/// where no present wait holds the CPU back and this retirement is the
/// only thing that does.
#[test]
fn the_loop_keeps_one_frame_executing_while_it_records_the_next() {
    let (mut shell, window, _recorder, mut gpu) =
        null_context("frames in flight test", Pacing::Off);

    for frame in 1..=5u64 {
        null_frame(&mut gpu).expect("present");
        let in_flight: Vec<u64> = gpu.in_flight.iter().map(|(value, _)| *value).collect();
        assert_eq!(
            in_flight,
            vec![frame],
            "after frame {frame}'s submit only it may still run"
        );
        assert!(
            in_flight.len() < FRAMES_IN_FLIGHT,
            "the ring would be overrun"
        );
    }

    gpu.destroy().expect("teardown");
    shell.destroy_window(window).expect("the window goes away");
}

/// **A deeper ring keeps more frames executing and waits further back**: the
/// depth the caller asks for is the depth both the retirement and the
/// present wait use, not the default.
#[test]
fn a_deeper_ring_keeps_more_frames_in_flight() {
    use crcbl_hal::null::{Event, NullInstance, Recorder};

    const DEPTH: usize = FRAMES_IN_FLIGHT + 1;
    let (shell, window) = shell();
    let recorder = Recorder::new();
    let pending = Presenter::request_open_on(
        Box::new(NullInstance::gpu_driven().with_recorder(recorder.clone())),
        &shell,
        window,
        (320, 240),
        &PresenterDesc {
            label: "deep ring test",
            pacing: Pacing::Vsync,
            frames_in_flight: DEPTH,
            ..PresenterDesc::default()
        },
    )
    .expect("the null backend opens everywhere");
    let mut gpu = finish_open(pending);
    assert_eq!(gpu.frames_in_flight(), DEPTH);

    const FRAMES: u64 = 6;
    for _ in 0..FRAMES {
        null_frame(&mut gpu).expect("present");
    }
    assert_eq!(
        gpu.in_flight.len(),
        DEPTH - 1,
        "all but one slot of the ring may still be executing"
    );
    let waited: Vec<u64> = recorder
        .events()
        .iter()
        .filter_map(|event| match event {
            Event::PresentWaited { present_id, .. } => Some(*present_id),
            _ => None,
        })
        .collect();
    let depth = DEPTH as u64;
    assert_eq!(
        waited,
        (1..=FRAMES - depth).collect::<Vec<_>>(),
        "the first {depth} frames wait for nothing and the rest wait {depth} back"
    );
    gpu.destroy().expect("teardown");
}

/// **A resize after presenting reconfigures with nothing in flight.** Every
/// command buffer the presenter submitted is destroyed before the swapchain
/// is reconfigured: D3D12's `ResizeBuffers` refuses while one still holds a
/// back buffer, which is what a bar resized in place after it had drawn hit.
#[test]
fn a_resize_after_presenting_reconfigures_with_nothing_in_flight() {
    use crcbl_hal::null::{Event, ObjectKind};

    let (mut shell, window, recorder, mut gpu) = null_context("resize drain test", Pacing::Vsync);
    for _ in 0..FRAMES_IN_FLIGHT + 1 {
        null_frame(&mut gpu).expect("present");
    }
    assert!(
        !gpu.in_flight.is_empty(),
        "frames are in flight before the resize, or this checks nothing"
    );

    gpu.resize((640, 480)).expect("a resize after presenting");
    assert!(gpu.in_flight.is_empty(), "nothing is left in flight");
    let events = recorder.events();
    let reconfigured = events
        .iter()
        .position(|event| {
            matches!(
                event,
                Event::Reconfigured {
                    extent: (640, 480),
                    ..
                }
            )
        })
        .expect("the resize reconfigured the swapchain");
    let before = &events[..reconfigured];
    let count = |kind: fn(&Event) -> bool| before.iter().filter(|event| kind(event)).count();
    let finished = count(|event| matches!(event, Event::Finished { .. }));
    let destroyed = count(|event| {
        matches!(
            event,
            Event::Destroyed {
                kind: ObjectKind::CommandBuffer
            }
        )
    });
    assert!(finished > 0);
    assert_eq!(
        destroyed, finished,
        "every command buffer is destroyed before the reconfigure"
    );

    null_frame(&mut gpu).expect("a frame at the new size");
    gpu.destroy().expect("teardown");
    shell.destroy_window(window).expect("the window goes away");
}

/// The runtime switch, against a backend that records what the swapchain
/// did: a pacing change rebuilds when — and only when — the present mode
/// moves.
///
/// # What this cannot see
///
/// That the display is asked **once** is structural (one
/// `observed_timing.is_some()` guard) and is not observable here: the null
/// backend answers `DisplayTiming::Unknown` however often it is asked, so a
/// run that queried every frame would record exactly the same events. What
/// is observable is the consequence — an `Auto` that resolves to vsync
/// costs no swapchain, and a later `Auto` settles against the sample
/// already taken rather than starting again from a request.
#[test]
fn a_pacing_switch_rebuilds_the_swapchain_only_when_the_mode_moves() {
    let (mut shell, window, recorder, mut gpu) =
        null_context("pacing switch test", Pacing::default());

    // Before any present there is no observation, so the request is `Auto`
    // and the answer is the vsync the swapchain was opened on.
    assert_eq!(gpu.pacing(), Pacing::Auto, "the default is Auto");
    assert_eq!(gpu.effective_pacing(), Pacing::Vsync);
    assert_eq!(gpu.config.present_mode, PresentMode::Fifo);

    let present = |gpu: &mut Presenter| {
        assert_eq!(null_frame(gpu).expect("present"), FrameOutcome::Presented,);
    };
    for _ in 0..3 {
        present(&mut gpu);
    }

    // The null display says `Unknown`, which is the fallback arm, so the
    // resolution changed nothing and spent nothing.
    assert_eq!(gpu.pacing(), Pacing::Auto, "the request is not rewritten");
    assert_eq!(gpu.effective_pacing(), Pacing::Vsync);
    assert_eq!(
        reconfigures(&recorder),
        0,
        "an Auto that resolved to vsync rebuilt the swapchain it was already using"
    );

    // Vsync by name is the mode already presenting: same answer, and a
    // settings screen re-applying it must not cost a swapchain.
    gpu.set_pacing(Pacing::Vsync).expect("switching to vsync");
    assert_eq!(gpu.pacing(), Pacing::Vsync);
    assert_eq!(gpu.effective_pacing(), Pacing::Vsync);
    assert_eq!(
        reconfigures(&recorder),
        0,
        "vsync over vsync rebuilt the swapchain"
    );

    // Off is a different mode on this surface — no `FifoRelaxed`, so
    // `Mailbox` — and that is what a rebuild is for.
    gpu.set_pacing(Pacing::Off).expect("switching to no sync");
    assert_eq!(gpu.effective_pacing(), Pacing::Off);
    assert_eq!(gpu.config.present_mode, PresentMode::Mailbox);
    assert_eq!(
        reconfigures(&recorder),
        1,
        "the mode moved and the swapchain did not"
    );

    gpu.set_pacing(Pacing::Off)
        .expect("switching to no sync again");
    assert_eq!(
        reconfigures(&recorder),
        1,
        "re-applying the mode in force rebuilt it again"
    );

    // Back to Auto: it settles against the observation already taken —
    // `Unknown`, so vsync — rather than asking the display a second time.
    gpu.set_pacing(Pacing::Auto)
        .expect("switching back to auto");
    assert_eq!(gpu.pacing(), Pacing::Auto);
    assert_eq!(gpu.effective_pacing(), Pacing::Vsync);
    assert_eq!(gpu.config.present_mode, PresentMode::Fifo);
    assert_eq!(reconfigures(&recorder), 2);

    // And the loop still runs on the swapchain all of that rebuilt.
    present(&mut gpu);

    gpu.destroy().expect("teardown");
    shell.destroy_window(window).expect("the window goes away");
}

/// The rollback when the swapchain cannot be rebuilt: a settings screen
/// whose apply fails must be left describing the swapchain that is still
/// configured, not the pacing that did not take.
///
/// The null backend never fails on its own, so the fault is injected
/// through the recorder — the same knob a reviewer would use to make the
/// rollback observable at all.
#[test]
fn a_failed_pacing_switch_rolls_back_the_request_the_effective_and_the_mode() {
    let (mut shell, window, recorder, mut gpu) =
        null_context("pacing rollback test", Pacing::default());

    // The presenter is on Auto -> vsync -> Fifo, and Off is a different mode
    // on this surface (no `FifoRelaxed`, so Mailbox), so the switch is the
    // one that has to rebuild — and therefore the one that can fail.
    assert_eq!(gpu.pacing(), Pacing::Auto);
    assert_eq!(gpu.effective_pacing(), Pacing::Vsync);
    assert_eq!(gpu.config.present_mode, PresentMode::Fifo);

    recorder.fail_next_reconfigures(1);
    let error = gpu
        .set_pacing(Pacing::Off)
        .expect_err("the injected fault must surface as an error");

    assert!(
        matches!(
            error,
            PresentError::Surface(SurfaceError::Hal(HalError::OutOfDeviceMemory))
        ),
        "{error}"
    );

    // The request, the effective pacing and the swapchain mode rolled back
    // together, rather than any of them describing the change that failed.
    assert_eq!(gpu.pacing(), Pacing::Auto, "the request is not rewritten");
    assert_eq!(
        gpu.effective_pacing(),
        Pacing::Vsync,
        "the effective pacing does not describe a swapchain that does not exist"
    );
    assert_eq!(
        gpu.config.present_mode,
        PresentMode::Fifo,
        "the swapchain is still the one that was configured"
    );
    assert_eq!(
        reconfigures(&recorder),
        0,
        "a failed reconfigure must not record itself as a success"
    );

    // The fault was consumed, not latched: the same switch succeeds now,
    // and the presenter stayed usable throughout.
    gpu.set_pacing(Pacing::Off)
        .expect("the fault was injected once, not forever");
    assert_eq!(gpu.effective_pacing(), Pacing::Off);
    assert_eq!(gpu.config.present_mode, PresentMode::Mailbox);

    gpu.destroy().expect("teardown");
    shell.destroy_window(window).expect("the window goes away");
}

/// An error the device reported **outside any call** stops the frame.
///
/// This is the failure mode P5.13 found in a browser and could not see from
/// the code: WebGPU hands back a pipeline object whose shader did not
/// compile and delivers the reason to the device's error channel later, so
/// every subsequent submit is silently discarded and the page draws nothing
/// while reporting itself healthy. `acquire` asks the device before it
/// records anything, which is the only point in a frame where that answer
/// can still change what happens.
#[test]
fn a_device_error_raised_between_calls_fails_the_next_frame() {
    let (mut shell, window, recorder, mut gpu) =
        null_context("device error test", Pacing::default());

    // The control: a device with nothing to report does not fail a frame.
    assert!(
        gpu.acquire().expect("a healthy device acquires").is_some(),
        "an unprovoked drain must not break every frame"
    );

    recorder.report_device_error("shader module 3 failed to compile");
    let error = gpu
        .acquire()
        .expect_err("the device reported an error before this frame");
    assert!(
        error.to_string().contains("shader module 3"),
        "the reason must survive to the caller, not just the fact: {error}"
    );

    // Reported once. A latched error would turn one bad pipeline into a
    // frame loop that can never run again.
    assert!(
        gpu.acquire()
            .expect("the error was already reported")
            .is_some(),
        "taking an error must clear it"
    );

    gpu.destroy().expect("teardown");
    shell.destroy_window(window).expect("the window goes away");
}

/// An error raised on the way out still reaches the caller.
///
/// `acquire` drains the out-of-band channel at the top of a frame, so
/// everything after the last one — the final submit completing, the
/// teardown calls themselves — had no reader at all, and a run that
/// violated the specification while shutting down exited 0.
#[test]
fn a_device_error_raised_during_teardown_fails_the_run() {
    let (mut shell, window, recorder, gpu) = null_context("teardown error", Pacing::default());

    recorder.report_device_error("the swapchain was destroyed while still in use");
    let error = gpu
        .destroy()
        .expect_err("the device had an error left to report");
    assert!(
        error.to_string().contains("destroyed while still in use"),
        "the reason must survive teardown, not just the fact: {error}"
    );

    shell.destroy_window(window).expect("the window goes away");
}

// ---- resize and device loss, injected -----------------------------------

/// A real [`Presenter`] on the null backend, and the recorder that decides
/// when its device misbehaves.
///
/// By hand rather than through the registry because the point is to *hold*
/// the recorder. The window is handed back with it because it outlives the
/// presenter and still has to be destroyed.
fn null_context(
    label: &str,
    pacing: Pacing,
) -> (
    HeadlessShell,
    WindowId,
    crcbl_hal::null::Recorder,
    Presenter,
) {
    null_context_with(
        crcbl_hal::null::NullInstance::gpu_driven(),
        label,
        pacing,
        Features::empty(),
    )
}

/// The same, on an adapter the caller chose and asking for `optional` on top
/// of nothing.
///
/// Which adapter and which features are one question, not two: the device
/// gets a capability only where the adapter has it *and* the open asked for
/// it, so a test that wants one has to say both, and saying only one is the
/// silent way to end up asserting against a device that never had it.
fn null_context_with(
    instance: crcbl_hal::null::NullInstance,
    label: &str,
    pacing: Pacing,
    optional: Features,
) -> (
    HeadlessShell,
    WindowId,
    crcbl_hal::null::Recorder,
    Presenter,
) {
    use crcbl_hal::null::Recorder;

    let (mut shell, window) = shell();
    let mut shell_events = 0;
    let extent =
        wait_for_configure(&mut shell, window, &mut |_| shell_events += 1).expect("configured");

    let recorder = Recorder::new();
    let pending = Presenter::request_open_on(
        Box::new(instance.with_recorder(recorder.clone())),
        &shell,
        window,
        extent,
        &PresenterDesc {
            label,
            optional_features: optional,
            pacing,
            ..PresenterDesc::default()
        },
    )
    .expect("the null backend opens everywhere");
    let gpu = finish_open(pending);
    (shell, window, recorder, gpu)
}

/// One frame on a null presenter, shaped like every caller's `draw`: acquire,
/// an empty command buffer, submit and present — with `Ok(None)` from
/// `acquire` reported as [`FrameOutcome::Reconfigured`], which is what every
/// sample does with it.
fn null_frame(gpu: &mut Presenter) -> Result<FrameOutcome, PresentError> {
    let Some(acquired) = gpu.acquire()? else {
        return Ok(FrameOutcome::Reconfigured);
    };
    let encoder = gpu
        .device()
        .create_command_encoder(&crcbl_hal::CommandEncoderDesc {
            label: Some("null frame"),
            queue: gpu.queue(),
        });
    let command_buffer = encoder.finish()?;
    gpu.submit_and_present(&acquired, command_buffer)
}

/// The start of the line [`Presenter::reconfigure`] writes.
///
/// A rebuild that *failed* records no event, so the recorder alone cannot
/// tell a presenter that never tried to rebuild from one that tried and was
/// refused — and on a lost device those are the two candidate policies.
/// This line is what tells them apart.
const RECONFIGURE_LINE: &str = "hal: reconfiguring the swapchain to ";

/// How many swapchain rebuilds the recorder saw.
fn reconfigures(recorder: &crcbl_hal::null::Recorder) -> usize {
    recorder
        .events()
        .iter()
        .filter(|event| matches!(event, crcbl_hal::null::Event::Reconfigured { .. }))
        .count()
}

/// How many presents actually reached the swapchain.
fn presents(recorder: &crcbl_hal::null::Recorder) -> usize {
    recorder
        .events()
        .iter()
        .filter(|event| matches!(event, crcbl_hal::null::Event::Presented { .. }))
        .count()
}

/// The presents and the rebuilds the recorder saw, in the order it saw
/// them.
///
/// What [`presents`] and [`reconfigures`] cannot answer: whether the rebuild
/// happened *after* the frame reached the display or instead of it. Both
/// orders leave the same two counts behind.
fn presentation_sequence(recorder: &crcbl_hal::null::Recorder) -> Vec<&'static str> {
    recorder
        .events()
        .iter()
        .filter_map(|event| match event {
            crcbl_hal::null::Event::Presented { .. } => Some("present"),
            crcbl_hal::null::Event::Reconfigured { .. } => Some("reconfigure"),
            _ => None,
        })
        .collect()
}

/// The whole of a paced frame as the recorder saw it: the pacing wait — with
/// one that lapsed named apart from one that was answered — then the
/// acquire, the present and any rebuild.
///
/// [`presentation_sequence`] cannot be used for a timed-out wait, because
/// the acquire is exactly what the candidate policies disagree about and it
/// does not appear there: a presenter that skipped the frame and one that
/// rendered it both leave a present-and-rebuild stream that is empty of
/// rebuilds.
fn paced_frame_sequence(recorder: &crcbl_hal::null::Recorder) -> Vec<&'static str> {
    recorder
        .events()
        .iter()
        .filter_map(|event| match event {
            crcbl_hal::null::Event::PresentWaited {
                timed_out: true, ..
            } => Some("wait timed out"),
            crcbl_hal::null::Event::PresentWaited { .. } => Some("wait"),
            crcbl_hal::null::Event::Acquired { .. } => Some("acquire"),
            crcbl_hal::null::Event::Presented { .. } => Some("present"),
            crcbl_hal::null::Event::Reconfigured { .. } => Some("reconfigure"),
            _ => None,
        })
        .collect()
}

/// **An acquire that reports the swapchain out of date is expected traffic,
/// not a failed frame**: it rebuilds and hands the frame back as skipped.
///
/// Reachable on a real driver only while someone drags a window edge, which
/// is why `crcbl-vk` had to carry the only test of it. The recorder's
/// out-of-date latch is what brings it to a machine with no GPU.
#[test]
fn an_out_of_date_acquire_reconfigures_the_swapchain_and_skips_the_frame() {
    let (mut shell, window, recorder, mut gpu) =
        null_context("out-of-date acquire test", Pacing::Vsync);

    // Nothing has been submitted, so no pacing wait is due and `acquire`
    // is the *only* call that can report this. The frame after it is the
    // one that would wait, and that case is its own test below.
    assert_eq!(
        Presenter::present_to_wait_for(
            gpu.submitted,
            gpu.effective_pacing(),
            gpu.frames_in_flight()
        ),
        None,
        "this test is about the acquire, so nothing else must be able to answer first"
    );

    recorder.report_swapchain_out_of_date();
    assert!(
        gpu.acquire()
            .expect("out of date is expected traffic, not an error")
            .is_none(),
        "an out-of-date acquire skips the frame; it does not hand out an image"
    );
    assert_eq!(
        reconfigures(&recorder),
        1,
        "the swapchain is rebuilt once, by the arm that caught it"
    );
    assert_eq!(presents(&recorder), 0, "a skipped frame presents nothing");

    // Handling it is what cleared it, so the next frame is an ordinary one.
    assert_eq!(
        null_frame(&mut gpu).expect("the rebuilt swapchain works"),
        FrameOutcome::Presented
    );
    assert_eq!(
        reconfigures(&recorder),
        1,
        "nothing rebuilt it a second time"
    );
    assert_eq!(presents(&recorder), 1);

    gpu.destroy().expect("teardown");
    shell.destroy_window(window).expect("the window goes away");
}

/// **Present is the usual place a resize is noticed**, and the presenter
/// says so in a comment; this is the assertion behind the comment.
///
/// The frame is recorded and submitted before the present refuses it, so
/// the outcome a loop records is [`FrameOutcome::Reconfigured`] — which is
/// what the engine's frame budget counts against its never-presents cap.
#[test]
fn an_out_of_date_present_rebuilds_and_reports_the_frame_as_reconfigured() {
    let (mut shell, window, recorder, mut gpu) =
        null_context("out-of-date present test", Pacing::Vsync);

    let acquired = gpu
        .acquire()
        .expect("acquire")
        .expect("the swapchain is healthy when the frame starts");
    // The resize lands after the image was handed out, which is the case
    // acquire cannot catch.
    recorder.report_swapchain_out_of_date();

    let encoder = gpu
        .device()
        .create_command_encoder(&crcbl_hal::CommandEncoderDesc {
            label: Some("out-of-date present test"),
            queue: gpu.queue(),
        });
    let command_buffer = encoder.finish().expect("an empty command buffer");
    assert_eq!(
        gpu.submit_and_present(&acquired, command_buffer)
            .expect("a resize noticed at present is not a failed frame"),
        FrameOutcome::Reconfigured
    );
    assert_eq!(reconfigures(&recorder), 1);
    assert_eq!(
        presents(&recorder),
        0,
        "the present that reported the swapchain out of date did not present"
    );

    assert_eq!(
        null_frame(&mut gpu).expect("the rebuilt swapchain works"),
        FrameOutcome::Presented
    );
    assert_eq!(reconfigures(&recorder), 1);
    assert_eq!(presents(&recorder), 1);

    gpu.destroy().expect("teardown");
    shell.destroy_window(window).expect("the window goes away");
}

/// **The pacing wait's own out-of-date arm**, which is the third and least
/// obvious of the three: it does nothing, deliberately, and leaves the
/// acquire behind it to reconfigure.
///
/// Without that arm the wait's error would propagate and a resize would
/// fail the frame — on a code path that only runs once the pipeline is
/// `FRAMES_IN_FLIGHT` deep and only on a display that is pacing, which is
/// why it needed a swapchain that can be made out of date on demand.
#[test]
fn an_out_of_date_pacing_wait_leaves_the_acquire_behind_it_to_reconfigure() {
    let (mut shell, window, recorder, mut gpu) =
        null_context("out-of-date wait test", Pacing::Vsync);

    // Fill the pipeline, so the next frame really does wait for an older
    // present rather than skipping the wait the way the first ones do.
    for _ in 0..=FRAMES_IN_FLIGHT {
        assert_eq!(
            null_frame(&mut gpu).expect("a healthy frame"),
            FrameOutcome::Presented
        );
    }
    assert!(
        Presenter::present_to_wait_for(
            gpu.submitted,
            gpu.effective_pacing(),
            gpu.frames_in_flight()
        )
        .is_some(),
        "the rest of this test asserts nothing unless the next frame is one that waits"
    );
    let waits = recorder
        .events()
        .iter()
        .filter(|event| matches!(event, crcbl_hal::null::Event::PresentWaited { .. }))
        .count();
    assert!(
        waits > 0,
        "and nothing unless the waits are actually reaching the device"
    );

    recorder.report_swapchain_out_of_date();
    assert!(
        gpu.acquire()
            .expect("a wait that reports a resize is not a failed frame")
            .is_none(),
        "the acquire behind it is what reconfigures and skips"
    );
    assert_eq!(reconfigures(&recorder), 1);
    assert_eq!(
        recorder
            .events()
            .iter()
            .filter(|event| matches!(event, crcbl_hal::null::Event::PresentWaited { .. }))
            .count(),
        waits,
        "the refused wait recorded nothing, so the arm ran on a real refusal"
    );

    gpu.destroy().expect("teardown");
    shell.destroy_window(window).expect("the window goes away");
}

/// **A lost device surfaces and stays surfaced.** The decision in
/// `docs/backlog.md` is that the engine does not rebuild its way out of one:
/// `HalError::DeviceLost` propagates, wearing the presentation vocabulary
/// the seam gives it, and every later frame says the same thing.
///
/// The contrast with `report_device_error` is the point. That one is
/// one-shot by contract — the test above asserts that taking it clears it —
/// so until the recorder could express a *permanent* loss, "this device is
/// gone and stays gone" had no test on any backend.
#[test]
fn a_lost_device_fails_every_frame_after_it_and_rebuilds_nothing() {
    let (mut shell, window, recorder, mut gpu) = null_context("device loss test", Pacing::Vsync);

    // The control: the same presenter, one healthy frame.
    assert_eq!(
        null_frame(&mut gpu).expect("a healthy frame"),
        FrameOutcome::Presented
    );
    let presented = presents(&recorder);
    assert_eq!(presented, 1);

    // A rebuild that *fails* records no event, so the recorder alone cannot
    // tell a presenter that never tried to rebuild from one that tried and
    // was refused — and those are the two policies. The log line can, and a
    // real resize first is what proves this capture would have seen one.
    let logs = crcbl_core::log::capture();
    let rebuilds_logged = || {
        logs.records()
            .iter()
            .filter(|record| record.message.starts_with(RECONFIGURE_LINE))
            .count()
    };
    gpu.resize((320, 240))
        .expect("a resize the device can still do");
    assert_eq!(rebuilds_logged(), 1, "{:?}", logs.records());
    let rebuilt = reconfigures(&recorder);
    assert_eq!(rebuilt, 1, "and the rebuild really happened");

    recorder.lose_device("gpu hang: the driver reset the adapter");

    let mut errors: Vec<PresentError> = Vec::new();
    for attempt in 0..3u32 {
        match null_frame(&mut gpu) {
            Ok(outcome) => panic!("frame {attempt} ran on a device that is gone: {outcome:?}"),
            Err(error) => errors.push(error),
        }
    }
    assert_eq!(
        errors.len(),
        3,
        "every frame after the loss failed, not just the first"
    );
    for error in &errors {
        assert!(
            matches!(
                error,
                PresentError::Surface(SurfaceError::Hal(HalError::DeviceLost(_)))
            ),
            "{error}"
        );
        assert!(
            error.to_string().contains("gpu hang"),
            "the driver's own words are what a player's log has to carry: {error}"
        );
    }

    assert_eq!(
        reconfigures(&recorder),
        rebuilt,
        "the presenter must not rebuild the swapchain out from under a lost device"
    );
    assert_eq!(
        rebuilds_logged(),
        1,
        "and must not so much as attempt one: {:?}",
        logs.records()
    );
    assert_eq!(
        presents(&recorder),
        presented,
        "and nothing reached the screen after the loss"
    );

    // Teardown reports it too rather than pretending the wait succeeded.
    let teardown = gpu
        .destroy()
        .expect_err("waiting for a dead device to go idle cannot succeed");
    assert!(
        matches!(teardown, PresentError::Hal(HalError::DeviceLost(_))),
        "{teardown}"
    );
    shell.destroy_window(window).expect("the window goes away");
}

/// **A suboptimal frame is presented and *then* the swapchain is rebuilt**,
/// and that order is the whole of what separates this arm from every other
/// reconfigure the presenter does.
///
/// The observable is the recorder's event *sequence*, not its counts,
/// because the counts cannot tell the candidate presenters apart. One that
/// ignores [`AcquiredFrame::suboptimal`] records the present and no rebuild.
/// One that treated it the way it treats
/// [`SurfaceError::OutOfDate`] would rebuild *instead* of presenting — a
/// rebuild with nothing on the display ahead of it, and a
/// [`FrameOutcome::Reconfigured`] for a frame that in fact went out. Only a
/// `present` followed by a `reconfigure`, on the one frame, is
/// [`Presenter::submit_and_present`] doing what it says.
///
/// `report_suboptimal_acquires` is *counted* rather than latched, so the
/// frame after it is an ordinary one again; the umbrella's driven test runs
/// the same report through a loop with a budget.
#[test]
fn a_suboptimal_frame_is_presented_and_then_the_swapchain_is_rebuilt() {
    let (mut shell, window, recorder, mut gpu) =
        null_context("suboptimal frame test", Pacing::Vsync);

    // The control: an undisturbed frame presents and rebuilds nothing, so
    // the sequence below is this one plus what the flag added.
    recorder.clear();
    assert_eq!(
        null_frame(&mut gpu).expect("a healthy frame"),
        FrameOutcome::Presented
    );
    assert_eq!(
        presentation_sequence(&recorder),
        ["present"],
        "nothing rebuilds a swapchain the surface still fits"
    );

    recorder.report_suboptimal_acquires(1);
    recorder.clear();
    assert_eq!(
        null_frame(&mut gpu).expect("a suboptimal frame is not a failed one"),
        FrameOutcome::Presented,
        "the frame reached the display, so the loop counts it as presented"
    );
    assert_eq!(
        presentation_sequence(&recorder),
        ["present", "reconfigure"],
        "the rebuild has to come after the present, not instead of it: {:?}",
        recorder.events()
    );

    // And it stops, because the report was spent by the acquire that read
    // it.
    recorder.clear();
    assert_eq!(
        null_frame(&mut gpu).expect("a healthy frame"),
        FrameOutcome::Presented
    );
    assert_eq!(
        presentation_sequence(&recorder),
        ["present"],
        "the rebuilt swapchain is not suboptimal again"
    );

    gpu.destroy().expect("teardown");
    shell.destroy_window(window).expect("the window goes away");
}

/// **A frame whose pacing wait timed out is still rendered and still
/// presented**, which is the whole of the policy that arm exists to carry
/// out: a frame skipped because the *last* one was late is two frames lost
/// instead of one.
///
/// The observable is the recorder's event sequence for the one frame, and
/// it has to separate three candidate presenters rather than two. One that
/// renders anyway leaves `wait timed out`, an `acquire` and a `present`.
/// One that treated the timeout the way it treats
/// [`SurfaceError::OutOfDate`] would leave the lapsed wait and **nothing
/// else** — no image was ever taken — and hand its caller
/// [`FrameOutcome::Reconfigured`] for a frame that never reached the
/// display. One that let the error propagate leaves that same truncated
/// sequence but fails the frame outright, so the outcome assertion is what
/// separates those two from each other and the sequence is what separates
/// both from the right answer.
///
/// Naming the lapsed wait apart from an answered one is the other half.
/// Every candidate above presents on a frame whose wait *succeeded*, so an
/// assertion that could not see the refusal would pass on an injection that
/// silently reached nothing — which is exactly what an injection on a device
/// without [`Features::PRESENT_FEEDBACK`] does, and why this presenter asks
/// for the capability on an adapter built to have it.
///
/// The frame after it is the seam's other claim — "let the next wait catch
/// up" — and it is testable only because
/// [`Recorder::report_present_wait_timeouts`](crcbl_hal::null::Recorder::report_present_wait_timeouts)
/// is counted rather than latched.
#[test]
fn a_timed_out_pacing_wait_still_renders_and_presents_the_frame() {
    use crcbl_hal::null::NullInstance;

    let (mut shell, window, recorder, mut gpu) = null_context_with(
        NullInstance::gpu_driven().with_present_feedback(),
        "timed-out wait test",
        Pacing::Vsync,
        Features::PRESENT_FEEDBACK,
    );
    assert!(
        gpu.device()
            .caps()
            .features
            .contains(Features::PRESENT_FEEDBACK),
        "a device that cannot observe presents cannot be told one was late, \
         and every assertion below would pass without the injection landing"
    );

    // Fill the pipeline: the first `FRAMES_IN_FLIGHT` frames have nothing
    // far enough behind them to wait for, so none of them would consume the
    // injection or exercise the arm.
    for _ in 0..=FRAMES_IN_FLIGHT {
        assert_eq!(
            null_frame(&mut gpu).expect("a healthy frame"),
            FrameOutcome::Presented
        );
    }
    assert!(
        Presenter::present_to_wait_for(
            gpu.submitted,
            gpu.effective_pacing(),
            gpu.frames_in_flight()
        )
        .is_some(),
        "the rest of this test asserts nothing unless the next frame is one that waits"
    );

    // The control: the same frame with its wait answered, so the sequence
    // below is this one with the wait's answer changed and nothing else.
    recorder.clear();
    assert_eq!(
        null_frame(&mut gpu).expect("a healthy frame"),
        FrameOutcome::Presented
    );
    assert_eq!(
        paced_frame_sequence(&recorder),
        ["wait", "acquire", "present"],
        "a paced frame waits, then takes an image, then presents it: {:?}",
        recorder.events()
    );

    recorder.report_present_wait_timeouts(1);
    recorder.clear();
    assert_eq!(
        null_frame(&mut gpu).expect("a display that is behind does not fail the frame"),
        FrameOutcome::Presented,
        "the frame reached the display, so the loop counts it as presented"
    );
    assert_eq!(
        paced_frame_sequence(&recorder),
        ["wait timed out", "acquire", "present"],
        "the frame is rendered anyway: a presenter that skipped or failed it \
         stops after the wait: {:?}",
        recorder.events()
    );
    assert_eq!(
        reconfigures(&recorder),
        0,
        "and a late display is not a resize, so nothing is rebuilt"
    );

    // The next wait catches up, which the arm's comment promises and only a
    // counted injection can show.
    recorder.clear();
    assert_eq!(
        null_frame(&mut gpu).expect("a healthy frame"),
        FrameOutcome::Presented
    );
    assert_eq!(
        paced_frame_sequence(&recorder),
        ["wait", "acquire", "present"],
        "the stall was one frame's, not this device's forever"
    );

    gpu.destroy().expect("teardown");
    shell.destroy_window(window).expect("the window goes away");
}

// ---- the submit's extra batch and its hook ------------------------------------

/// **What rides with a frame is submitted in the frame's own batch, after
/// it, and the hook runs after the present and before a suboptimal
/// rebuild.**
///
/// The seam the engine's `--screenshot` uses: the copy out of the presented
/// image must be in the same submission as the frame that drew it, and its
/// bytes are read once the present has answered — on the image that was
/// presented, so before a rebuild could replace it.
#[test]
fn a_submit_carries_what_rides_with_it_and_reports_the_present_to_its_hook() {
    use crcbl_hal::null::Event;

    let (mut shell, window, recorder, mut gpu) = null_context("extra batch test", Pacing::Vsync);
    recorder.report_suboptimal_acquires(1);

    let acquired = gpu.acquire().expect("acquire").expect("a frame");
    let record = |gpu: &Presenter, label| {
        gpu.device()
            .create_command_encoder(&crcbl_hal::CommandEncoderDesc {
                label: Some(label),
                queue: gpu.queue(),
            })
            .finish()
            .expect("an empty command buffer")
    };
    let frame = record(&gpu, "the frame");
    let copy = record(&gpu, "the copy");
    recorder.clear();

    let mut seen = Vec::new();
    let outcome = gpu
        .submit_and_present_with::<PresentError>(&acquired, frame, &[copy], |device, outcome| {
            seen.push((outcome, presentation_sequence(&recorder)));
            device.destroy_command_buffer(copy);
            Ok(())
        })
        .expect("present");
    assert_eq!(outcome, FrameOutcome::Presented);
    assert_eq!(
        seen,
        [(FrameOutcome::Presented, vec!["present"])],
        "the hook ran once, after the present and before the rebuild"
    );
    assert_eq!(
        presentation_sequence(&recorder),
        ["present", "reconfigure"],
        "and the suboptimal rebuild still came after it"
    );
    let batches: Vec<Vec<CommandBufferHandle>> = recorder
        .events()
        .iter()
        .filter_map(|event| match event {
            Event::Submitted {
                command_buffers, ..
            } => Some(command_buffers.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        batches,
        [vec![frame, copy]],
        "one submission, the frame first and what rides with it after"
    );
    // The suboptimal rebuild drains the ring, so what joined it is read from
    // what was destroyed: the copy once, by the hook, and the frame once, by
    // the drain. A copy that had joined the ring would be destroyed again.
    assert!(gpu.in_flight.is_empty(), "the rebuild drained the ring");
    let destroyed = recorder
        .events()
        .iter()
        .filter(|event| {
            matches!(
                event,
                Event::Destroyed {
                    kind: crcbl_hal::null::ObjectKind::CommandBuffer
                }
            )
        })
        .count();
    assert_eq!(
        destroyed, 2,
        "only the frame's own buffer joins the ring; the rest are the caller's"
    );

    gpu.destroy().expect("teardown");
    shell.destroy_window(window).expect("the window goes away");
}

/// **An out-of-date present reaches the hook before the rebuild, and an
/// error from the hook ends the frame there.**
///
/// The engine's `--screenshot` refuses a frame that never reached anything
/// rather than retrying it later; that refusal has to come before the
/// swapchain is rebuilt, or the run would rebuild and then fail.
#[test]
fn an_out_of_date_present_reaches_the_hook_before_the_rebuild() {
    let (mut shell, window, recorder, mut gpu) = null_context("hook refusal test", Pacing::Vsync);

    let acquired = gpu.acquire().expect("acquire").expect("a frame");
    recorder.report_swapchain_out_of_date();
    let frame = gpu
        .device()
        .create_command_encoder(&crcbl_hal::CommandEncoderDesc {
            label: Some("hook refusal test"),
            queue: gpu.queue(),
        })
        .finish()
        .expect("an empty command buffer");
    let refused = gpu
        .submit_and_present_with(&acquired, frame, &[], |_, outcome| {
            assert_eq!(outcome, FrameOutcome::Reconfigured);
            Err(PresentError::Unusable("the caller refused this frame"))
        })
        .expect_err("the hook's refusal ends the frame");
    assert!(
        matches!(
            refused,
            PresentError::Unusable("the caller refused this frame")
        ),
        "{refused}"
    );
    assert_eq!(
        reconfigures(&recorder),
        0,
        "nothing ran after the hook refused, so nothing was rebuilt"
    );

    gpu.destroy().expect("teardown");
    shell.destroy_window(window).expect("the window goes away");
}

// ---- the present-wait span ---------------------------------------------------

/// Runs `body` in a process of its own, with the trace on.
///
/// **A mutex would not be enough here.** The gate, the per-thread buffers
/// and [`crcbl_core::trace::drain`] are all process-wide and `cargo test`
/// runs this binary's tests as threads, so any other test's frame, running
/// concurrently while the gate is on, would record into the same buffers.
/// A child process is the only isolation that actually holds, and it turns
/// the gate on the way a user does, through `CRCBL_TRACE`. The `crcbl`
/// umbrella's engine tests carry the same helper for its loop's spans.
///
/// `name` is this test's full path. It is checked rather than trusted — see
/// the assertion on the child's report.
fn in_a_traced_process(name: &str, body: impl FnOnce()) {
    if std::env::var(crcbl_core::trace::ENV_VAR).is_ok() {
        assert!(
            crcbl_core::trace::init_from_env(),
            "the child is launched with the trace on"
        );
        drop(crcbl_core::trace::drain());
        body();
        return;
    }

    let output =
        crcbl_spawn::command(std::env::current_exe().expect("a test binary knows its own path"))
            .args(["--exact", name])
            .env(crcbl_core::trace::ENV_VAR, "1")
            .output()
            .expect("re-running this test binary");
    let report = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "the traced child failed:\n{report}"
    );
    // `--exact` with a name that matches nothing exits zero, so without this
    // a rename would leave the whole test passing on nothing at all.
    assert!(
        report.contains("1 passed"),
        "the child ran no test — is {name:?} still this test's path?\n{report}"
    );
}

/// A record reduced to what these assertions are written against.
fn span_shapes(
    snapshot: &crcbl_core::trace::Snapshot,
) -> Vec<(&'static str, crcbl_core::trace::RecordKind, u16)> {
    snapshot
        .threads
        .iter()
        .flat_map(|thread| &thread.records)
        .map(|record| (record.name, record.kind, record.depth))
        .collect()
}

/// **The swapchain wait is a span of its own**, so the CPU frame time can
/// have it taken back out. It is what a vsynced frame spends most of itself
/// in, and counting it as CPU work is how a row answers "CPU-bound" to every
/// frame on every machine.
#[test]
fn the_swapchain_wait_is_recorded_as_a_span_of_its_own() {
    in_a_traced_process(
        "present::tests::the_swapchain_wait_is_recorded_as_a_span_of_its_own",
        || {
            use crcbl_core::trace::RecordKind::{SpanBegin, SpanEnd};

            let (_shell, _window, _recorder, mut gpu) =
                null_context("present-wait span test", Pacing::Vsync);
            // Two frames: the first has nothing submitted to wait on, the
            // second does, and both go through the acquire. The second's
            // submit also retires the first — the loop keeps one frame in
            // flight — and that wait is a span too: three in all.
            null_frame(&mut gpu).expect("the null backend presents");
            null_frame(&mut gpu).expect("the null backend presents");

            let snapshot = crcbl_core::trace::drain();
            assert_eq!(
                span_shapes(&snapshot),
                vec![
                    (PRESENT_WAIT_SPAN, SpanBegin, 0),
                    (PRESENT_WAIT_SPAN, SpanEnd, 0),
                    (PRESENT_WAIT_SPAN, SpanBegin, 0),
                    (PRESENT_WAIT_SPAN, SpanEnd, 0),
                    (PRESENT_WAIT_SPAN, SpanBegin, 0),
                    (PRESENT_WAIT_SPAN, SpanEnd, 0),
                ],
                "a hand-written loop opens no frame span, so these sit at depth zero",
            );
            gpu.destroy().expect("the null backend releases everything");
        },
    );
}

#[test]
fn command_retirement_waits_are_excluded_from_cpu_work() {
    in_a_traced_process(
        "present::tests::command_retirement_waits_are_excluded_from_cpu_work",
        || {
            use crcbl_core::trace::RecordKind::{SpanBegin, SpanEnd};
            let (_shell, _window, _recorder, mut gpu) =
                null_context("retirement wait spans", Pacing::Vsync);
            null_frame(&mut gpu).expect("first frame");
            null_frame(&mut gpu).expect("second frame");
            // The loop keeps one submission in flight, so a second
            // outstanding one is queued by hand: two waits are what this
            // test needs to tell one span per wait from one per call.
            let extra = gpu
                .device()
                .create_command_encoder(&crcbl_hal::CommandEncoderDesc {
                    label: Some("retirement wait spans"),
                    queue: gpu.queue(),
                })
                .finish()
                .expect("an empty command buffer");
            let submitted = gpu.submitted;
            gpu.in_flight.push_back((submitted, extra));
            assert_eq!(gpu.in_flight.len(), 2, "two submissions to retire");
            drop(crcbl_core::trace::drain());
            gpu.retire_to(0).expect("retire both submissions");
            assert_eq!(
                span_shapes(&crcbl_core::trace::drain()),
                vec![
                    (PRESENT_WAIT_SPAN, SpanBegin, 0),
                    (PRESENT_WAIT_SPAN, SpanEnd, 0),
                    (PRESENT_WAIT_SPAN, SpanBegin, 0),
                    (PRESENT_WAIT_SPAN, SpanEnd, 0),
                ],
                "each blocking wait needs its own CPU-idle exclusion"
            );
            gpu.destroy().expect("release resources");
        },
    );
}

/// **`Transparency::Preferred` presents a transparent window opaque only when
/// no adapter can composite it, and says so.**
///
/// Red when the fallback is taken while a later adapter offers premultiplied
/// alpha, when it is not taken at all, or when `Presenter::composite_alpha`
/// reports anything but what the swapchain was built with.
#[test]
fn a_preferred_transparency_falls_back_to_opaque_only_when_it_must() {
    use crcbl_hal::AdapterId;
    use crcbl_hal::null::NullInstance;

    let opaque = vec![CompositeAlpha::Opaque];
    let both = vec![CompositeAlpha::Opaque, CompositeAlpha::PreMultiplied];
    let (_shell, mut gpu) = open_alpha_context_with(
        alpha_instance(
            NullInstance::gpu_driven().with_adapters(2),
            &[opaque.clone(), both],
        ),
        wayland_target(true),
        None,
        Transparency::Preferred,
    )
    .expect("the second adapter composites the window");
    assert_eq!(gpu.adapter, AdapterId(1), "premultiplied wins over opaque");
    assert_eq!(gpu.composite_alpha(), CompositeAlpha::PreMultiplied);
    gpu.drain().expect("nothing was submitted");
    gpu.destroy().expect("teardown");

    let (_shell, mut gpu) = open_alpha_context_with(
        alpha_instance(
            NullInstance::gpu_driven().with_adapters(2),
            &[opaque.clone(), opaque],
        ),
        wayland_target(true),
        None,
        Transparency::Preferred,
    )
    .expect("no adapter composites the window, and opaque was allowed");
    assert_eq!(gpu.adapter, AdapterId(0), "the first adapter that serves");
    assert_eq!(gpu.composite_alpha(), CompositeAlpha::Opaque);
    assert_eq!(
        gpu.config.desc(gpu.surface).composite_alpha,
        CompositeAlpha::Opaque,
        "the swapchain is built opaque"
    );
    gpu.drain().expect("nothing was submitted");
    gpu.destroy().expect("teardown");
}
