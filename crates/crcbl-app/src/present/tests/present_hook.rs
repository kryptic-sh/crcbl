//! The window's present hook told about every present: the size each frame
//! was drawn at, and only the presents that went through.

use super::*;

use crcbl_hal::null::NullInstance;
use crcbl_shell::PhysicalSize;

/// `extent` as the hook is told it.
fn size_of(extent: (u32, u32)) -> PhysicalSize {
    PhysicalSize::new(extent.0, extent.1)
}

/// Acquires a frame, which this test needs to have been handed over.
fn acquired(gpu: &mut Presenter) -> AcquiredFrame {
    gpu.acquire().expect("acquire").expect("a frame")
}

/// Presents `frame` with an empty command buffer.
fn present(gpu: &mut Presenter, frame: &AcquiredFrame) -> FrameOutcome {
    let commands = gpu
        .device()
        .create_command_encoder(&crcbl_hal::CommandEncoderDesc {
            label: Some("present hook test"),
            queue: gpu.queue(),
        })
        .finish()
        .expect("an empty command buffer");
    gpu.submit_and_present(frame, commands).expect("present")
}

/// **Each present is announced at the extent it was acquired at**, the one
/// after a resize at the new one.
#[test]
fn every_present_tells_the_hook_the_extent_it_was_drawn_at() {
    let (mut shell, window, _recorder, mut gpu) =
        null_context("present hook extent test", Pacing::Vsync);
    assert_eq!(shell.last_presented(window).expect("live"), None);

    let frame = acquired(&mut gpu);
    assert_eq!(present(&mut gpu, &frame), FrameOutcome::Presented);
    assert_eq!(
        shell.last_presented(window).expect("live"),
        Some(size_of(frame.extent))
    );

    gpu.resize((200, 100)).expect("resize");
    let frame = acquired(&mut gpu);
    assert_eq!(frame.extent, (200, 100), "the resize took");
    assert_eq!(present(&mut gpu, &frame), FrameOutcome::Presented);
    assert_eq!(
        shell.last_presented(window).expect("live"),
        Some(PhysicalSize::new(200, 100))
    );

    gpu.destroy().expect("teardown");
    shell.destroy_window(window).expect("the window goes away");
}

/// **A present that went out of date is announced and never confirmed**, so
/// the last present the shell knows of is still the one before it.
#[test]
fn an_out_of_date_present_is_not_confirmed() {
    let (mut shell, window, recorder, mut gpu) =
        null_context("present hook out-of-date test", Pacing::Vsync);
    let first = acquired(&mut gpu);
    assert_eq!(present(&mut gpu, &first), FrameOutcome::Presented);

    // At another size, so the announcement alone would show if it were
    // taken as confirmed.
    gpu.resize((200, 100)).expect("resize");
    let frame = acquired(&mut gpu);
    assert_ne!(frame.extent, first.extent);
    recorder.report_swapchain_out_of_date();
    assert_eq!(present(&mut gpu, &frame), FrameOutcome::Reconfigured);
    assert_eq!(
        shell.last_presented(window).expect("live"),
        Some(size_of(first.extent)),
        "the out-of-date present is not one the shell is told went through"
    );

    gpu.destroy().expect("teardown");
    shell.destroy_window(window).expect("the window goes away");
}

/// **A null device presenting to a real window takes no hook**: its presents
/// commit nothing, so state a shell held for one would never land. The same
/// open on the headless window's own target keeps it, which is what shows the
/// hook was there to take.
#[test]
fn a_null_device_presenting_to_a_real_window_takes_no_hook() {
    let run = |wayland: bool| {
        let (mut shell, window) = super::shell();
        let target = if wayland {
            wayland_target(false)
        } else {
            shell
                .surface_target(window)
                .expect("the headless window is alive")
        };
        let mut pending = request(
            target,
            (320, 240),
            "null device present hook test",
            Pacing::Off,
            Features::empty(),
            None,
        );
        pending.window = Some(window);
        pending.present_hook = shell.present_hook(window).expect("live");
        assert!(pending.present_hook.is_some(), "the shell has a hook");
        pending.stage = Presenter::start_device(Box::new(NullInstance::gpu_driven()), &pending)
            .expect("the null backend starts");
        let mut gpu = finish_open(pending);
        let frame = acquired(&mut gpu);
        assert_eq!(present(&mut gpu, &frame), FrameOutcome::Presented);
        gpu.destroy().expect("teardown");
        let presented = shell.last_presented(window).expect("live");
        shell.destroy_window(window).expect("the window goes away");
        presented
    };
    assert_eq!(run(true), None, "no hook, so nothing was told");
    assert_eq!(run(false), Some(PhysicalSize::new(320, 240)));
}

/// **A window opened on a shared device has its own hook**, told about its
/// own presents and nobody else's.
#[test]
fn a_shared_presenter_tells_its_own_windows_hook() {
    let (mut shell, first, _recorder, gpu) =
        null_context("present hook shared test", Pacing::Vsync);
    let second = shell
        .create_window(&crcbl_shell::WindowDesc::default())
        .expect("a second window");
    let mut beside = gpu
        .shared()
        .open(
            &shell,
            second,
            (200, 100),
            &PresenterDesc {
                label: "present hook shared test, second window",
                ..PresenterDesc::default()
            },
        )
        .expect("the shared device presents to a headless window");

    let frame = acquired(&mut beside);
    assert_eq!(present(&mut beside, &frame), FrameOutcome::Presented);
    assert_eq!(
        shell.last_presented(second).expect("live"),
        Some(size_of(frame.extent))
    );
    assert_eq!(
        shell.last_presented(first).expect("live"),
        None,
        "the first window presented nothing"
    );

    beside.destroy().expect("teardown");
    gpu.destroy().expect("teardown");
    shell.destroy_window(second).expect("the window goes away");
    shell.destroy_window(first).expect("the window goes away");
}
