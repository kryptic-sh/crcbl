//! A presenter paced by frame requests: no second present until the first
//! one's [`ShellEvent::FrameReady`] is observed, and nothing that can leave
//! the gate shut for good.

use super::*;

use crcbl_hal::CommandEncoderDesc;
use crcbl_hal::null::{Event, NullInstance, Recorder};
use crcbl_shell::ShellCaps;

/// A headless shell reporting `caps`, one window on it, and a presenter on
/// that window over a null device that records into `recorder`.
fn open_paced(caps: ShellCaps, recorder: &Recorder) -> (HeadlessShell, WindowId, Presenter) {
    let mut shell = HeadlessShell::new().with_caps(caps);
    let window = shell
        .create_window(&crcbl_shell::WindowDesc::default())
        .expect("headless always creates a window");
    let pending = Presenter::request_open_on(
        Box::new(NullInstance::gpu_driven().with_recorder(recorder.clone())),
        &shell,
        window,
        (320, 240),
        &PresenterDesc {
            label: "frame gate test",
            ..PresenterDesc::default()
        },
    )
    .expect("the null backend opens everywhere");
    (shell, window, finish_open(pending))
}

/// How many images the device has handed out.
fn acquires(recorder: &Recorder) -> usize {
    recorder
        .events()
        .iter()
        .filter(|event| matches!(event, Event::Acquired { .. }))
        .count()
}

/// One pump into the presenter.
fn pump(shell: &mut HeadlessShell, gpu: &mut Presenter) {
    shell.pump(&mut |event| gpu.observe(&event));
}

/// Records and presents `frame`, requesting the next one first when `shell`
/// is given.
fn present(
    gpu: &mut Presenter,
    frame: &AcquiredFrame,
    shell: Option<&mut HeadlessShell>,
) -> FrameOutcome {
    let encoder = gpu.device().create_command_encoder(&CommandEncoderDesc {
        label: Some("frame gate test"),
        queue: gpu.queue(),
    });
    let commands = encoder.finish().expect("an empty command buffer");
    if let Some(shell) = shell {
        gpu.request_frame(shell).expect("the window is live");
    }
    gpu.submit_and_present(frame, commands).expect("present")
}

/// The loop the module docs give, `frames` times: pump, acquire, request,
/// present. Returns how many frames were presented.
fn paced_loop(shell: &mut HeadlessShell, gpu: &mut Presenter, frames: usize) -> usize {
    let mut presented = 0;
    for _ in 0..frames {
        pump(shell, gpu);
        let Some(frame) = gpu.acquire().expect("acquire") else {
            continue;
        };
        assert_eq!(present(gpu, &frame, Some(shell)), FrameOutcome::Presented);
        presented += 1;
    }
    presented
}

/// **The fix for a status bar on an output that is off.** Where the
/// compositor paces frames, a present that carried a request is followed by
/// no acquire — the call that blocks there — until the answer is observed.
#[test]
fn a_present_that_carried_a_request_waits_for_its_answer() {
    let recorder = Recorder::new();
    let (mut shell, window, mut gpu) = open_paced(ShellCaps::DESKTOP, &recorder);
    assert!(
        shell.caps().contains(ShellCaps::FRAME_CALLBACK),
        "the fixture must be the compositor-paced case"
    );

    let frame = gpu.acquire().expect("acquire").expect("the first frame");
    assert_eq!(
        present(&mut gpu, &frame, Some(&mut shell)),
        FrameOutcome::Presented
    );
    assert!(shell.frame_requested(window).expect("live"), "asked");
    assert!(!gpu.frame_due(), "the gate closed on the present");

    let before = acquires(&recorder);
    assert!(
        gpu.acquire().expect("acquire").is_none(),
        "no frame before the answer"
    );
    assert_eq!(acquires(&recorder), before, "and the device was not asked");

    pump(&mut shell, &mut gpu);
    assert!(gpu.frame_due(), "the answer reopened the gate");
    let frame = gpu.acquire().expect("acquire").expect("a frame after it");
    assert_eq!(acquires(&recorder), before + 1);
    assert_eq!(
        present(&mut gpu, &frame, Some(&mut shell)),
        FrameOutcome::Presented
    );
    gpu.destroy().expect("teardown");
}

/// **One code path, and the cadence it keeps.** A loop that pumps once a
/// frame presents every frame with the capability set or clear, and a caller
/// that never requests a frame is not gated at all.
#[test]
fn a_loop_that_pumps_every_frame_keeps_its_cadence() {
    /// Frames each loop below runs.
    const FRAMES: usize = 4;

    for caps in [
        ShellCaps::DESKTOP,
        ShellCaps::DESKTOP.difference(ShellCaps::FRAME_CALLBACK),
    ] {
        let recorder = Recorder::new();
        let (mut shell, _window, mut gpu) = open_paced(caps, &recorder);
        assert_eq!(
            paced_loop(&mut shell, &mut gpu, FRAMES),
            FRAMES,
            "every frame presented with caps {caps:?}"
        );
        gpu.destroy().expect("teardown");
    }

    let recorder = Recorder::new();
    let (_shell, _window, mut gpu) = open_paced(ShellCaps::DESKTOP, &recorder);
    for _ in 0..FRAMES {
        let frame = gpu
            .acquire()
            .expect("acquire")
            .expect("an unrequested presenter is never gated");
        assert_eq!(present(&mut gpu, &frame, None), FrameOutcome::Presented);
    }
    gpu.destroy().expect("teardown");
}

/// **A destroyed window never holds the gate shut.** No answer comes for it,
/// so its destruction is the answer; and a request for it fails without
/// touching the gate.
#[test]
fn a_destroyed_window_reopens_the_gate() {
    let recorder = Recorder::new();
    let (mut shell, window, mut gpu) = open_paced(ShellCaps::DESKTOP, &recorder);
    let frame = gpu.acquire().expect("acquire").expect("the first frame");
    present(&mut gpu, &frame, Some(&mut shell));
    assert!(!gpu.frame_due(), "a request is outstanding");

    shell.destroy_window(window).expect("the window goes away");
    pump(&mut shell, &mut gpu);
    assert!(gpu.frame_due(), "the destroyed window let the gate go");
    assert!(
        matches!(
            gpu.request_frame(&mut shell),
            Err(ShellError::InvalidWindow { .. })
        ),
        "a gone window cannot be asked for"
    );
    assert!(gpu.frame_due(), "and the refusal left the gate open");
    // A refused request is not one a present can carry: the next present
    // must not wait for an answer nobody owes.
    let frame = gpu.acquire().expect("acquire").expect("a frame");
    assert_eq!(present(&mut gpu, &frame, None), FrameOutcome::Presented);
    assert!(gpu.frame_due(), "the refused request closed nothing");
    gpu.destroy().expect("teardown");
}

/// **Only this window's answer counts.** Another window's `FrameReady`
/// leaves the gate shut.
#[test]
fn another_windows_answer_is_not_this_ones() {
    let recorder = Recorder::new();
    let (mut shell, _window, mut gpu) = open_paced(ShellCaps::DESKTOP, &recorder);
    let other = shell
        .create_window(&crcbl_shell::WindowDesc::default())
        .expect("a second window");
    let frame = gpu.acquire().expect("acquire").expect("the first frame");
    present(&mut gpu, &frame, Some(&mut shell));

    gpu.observe(&ShellEvent::FrameReady { window: other });
    gpu.observe(&ShellEvent::WindowDestroyed { window: other });
    assert!(!gpu.frame_due(), "neither event was about this window");
    gpu.destroy().expect("teardown");
}

/// **A present that did not reach the swapchain owes no answer**, so it does
/// not close the gate — and the request it would have carried rides the next
/// present instead.
#[test]
fn a_present_that_went_out_of_date_leaves_the_gate_open() {
    let recorder = Recorder::new();
    let (mut shell, _window, mut gpu) = open_paced(ShellCaps::DESKTOP, &recorder);
    let frame = gpu.acquire().expect("acquire").expect("the first frame");
    recorder.report_swapchain_out_of_date();
    assert_eq!(
        present(&mut gpu, &frame, Some(&mut shell)),
        FrameOutcome::Reconfigured
    );
    assert!(gpu.frame_due(), "nothing was committed, so nothing is owed");

    let frame = gpu
        .acquire()
        .expect("acquire")
        .expect("the rebuilt swapchain hands over a frame");
    assert_eq!(present(&mut gpu, &frame, None), FrameOutcome::Presented);
    assert!(
        !gpu.frame_due(),
        "the earlier request rode this present, so its answer is awaited"
    );
    gpu.destroy().expect("teardown");
}

/// **An offscreen presenter has no window to ask for**, so a request does
/// nothing and nothing waits.
#[test]
fn an_offscreen_presenter_ignores_frame_requests() {
    let mut shell = HeadlessShell::new();
    let mut gpu = Presenter::open_offscreen(
        (64, 32),
        &PresenterDesc {
            label: "offscreen frame gate test",
            backend: Some(GpuBackend::Null),
            ..PresenterDesc::default()
        },
    )
    .expect("the null backend opens everywhere");
    for _ in 0..2 {
        let frame = gpu
            .acquire()
            .expect("acquire")
            .expect("an offscreen presenter is never gated");
        assert_eq!(
            present(&mut gpu, &frame, Some(&mut shell)),
            FrameOutcome::Presented
        );
    }
    gpu.destroy().expect("teardown");
}
