//! Several windows on one device through [`SharedGpu`]: the device is shared,
//! each window's swapchain is its own, and no window's close takes the device
//! from the others.

use super::*;

use crcbl_hal::CommandEncoderDesc;
use crcbl_hal::null::{Event, NullInstance, ObjectKind, Recorder};

/// A headless shell with `windows` windows, and a presenter on the first over
/// a null device that records into `recorder`.
fn open_first(windows: usize, recorder: &Recorder) -> (HeadlessShell, Vec<WindowId>, Presenter) {
    let mut shell = HeadlessShell::new();
    let windows: Vec<WindowId> = (0..windows)
        .map(|_| {
            shell
                .create_window(&crcbl_shell::WindowDesc::default())
                .expect("headless always creates a window")
        })
        .collect();
    let pending = Presenter::request_open_on(
        Box::new(NullInstance::gpu_driven().with_recorder(recorder.clone())),
        &shell,
        windows[0],
        (320, 240),
        &PresenterDesc {
            label: "first window",
            ..PresenterDesc::default()
        },
    )
    .expect("the null backend opens everywhere");
    (shell, windows, finish_open(pending))
}

/// The second window's descriptor.
fn beside() -> PresenterDesc<'static> {
    PresenterDesc {
        label: "second window",
        ..PresenterDesc::default()
    }
}

/// Acquires, records nothing, and presents one frame.
fn one_frame(gpu: &mut Presenter) {
    let frame = gpu
        .acquire()
        .expect("acquire")
        .expect("an image is available");
    let commands = gpu
        .device()
        .create_command_encoder(&CommandEncoderDesc {
            label: Some("shared test"),
            queue: gpu.queue(),
        })
        .finish()
        .expect("an empty command buffer");
    gpu.submit_and_present(&frame, commands).expect("present");
}

/// How many swapchains `recorder` saw made and torn down.
fn swapchains(recorder: &Recorder) -> (usize, usize) {
    let events = recorder.events();
    let count = |destroyed: bool| {
        events
            .iter()
            .filter(|event| match event {
                Event::Created { kind, .. } => !destroyed && *kind == ObjectKind::Swapchain,
                Event::Destroyed { kind } => destroyed && *kind == ObjectKind::Swapchain,
                _ => false,
            })
            .count()
    };
    (count(false), count(true))
}

/// **A window opened on a shared handle draws on the same device, through a
/// swapchain of its own**, and presents beside the first.
#[test]
fn a_second_window_shares_the_device_and_not_the_swapchain() {
    let recorder = Recorder::new();
    let (shell, windows, mut first) = open_first(2, &recorder);
    let mut second = first
        .shared()
        .open(&shell, windows[1], (200, 100), &beside())
        .expect("the null adapter presents to any window");
    assert!(
        core::ptr::addr_eq(first.device(), second.device()),
        "the two presenters are on one device"
    );
    assert_ne!(first.swapchain(), second.swapchain());
    one_frame(&mut first);
    one_frame(&mut second);
    assert_eq!(swapchains(&recorder), (2, 0));
    first.destroy().expect("destroy");
    second.destroy().expect("destroy");
    assert_eq!(swapchains(&recorder), (2, 2));
}

/// **Closing the window the device was opened for leaves the other drawing**:
/// its swapchain goes, the device stays for the window still on it — the
/// teardown NVIDIA's driver crashes in when each window has a device of its
/// own.
#[test]
fn closing_the_first_window_leaves_the_second_drawing() {
    let recorder = Recorder::new();
    let (shell, windows, first) = open_first(2, &recorder);
    let mut second = first
        .shared()
        .open(&shell, windows[1], (200, 100), &beside())
        .expect("the null adapter presents to any window");
    first.destroy().expect("destroy");
    assert_eq!(
        swapchains(&recorder),
        (2, 1),
        "only the first swapchain went"
    );
    one_frame(&mut second);
    one_frame(&mut second);
    second.destroy().expect("destroy");
}

/// **A handle outlives every window it opened**: with all of them closed it
/// still opens the next one on the same device — a toast after the bars that
/// opened the device have gone.
#[test]
fn a_handle_opens_a_window_after_every_other_has_closed() {
    let recorder = Recorder::new();
    let (shell, windows, first) = open_first(2, &recorder);
    let shared = first.shared();
    first.destroy().expect("destroy");
    let mut later = shared
        .open(&shell, windows[1], (200, 100), &beside())
        .expect("the device is still open");
    one_frame(&mut later);
    later.destroy().expect("destroy");
    assert_eq!(swapchains(&recorder), (2, 2));
}

/// **A window that requires a feature the shared device lacks is refused**,
/// not opened on a device that cannot serve it.
#[test]
fn a_required_feature_the_shared_device_lacks_is_refused() {
    let recorder = Recorder::new();
    let (shell, windows, first) = open_first(2, &recorder);
    let missing = Features::all() - first.device().caps().features;
    assert!(
        !missing.is_empty(),
        "the null preset lacks something to ask for"
    );
    let refused = first.shared().open(
        &shell,
        windows[1],
        (200, 100),
        &PresenterDesc {
            required_features: missing,
            ..beside()
        },
    );
    assert!(matches!(refused, Err(PresentError::Unusable(_))));
    assert_eq!(
        swapchains(&recorder),
        (1, 0),
        "nothing was made for the refused window"
    );
    first.destroy().expect("destroy");
}
