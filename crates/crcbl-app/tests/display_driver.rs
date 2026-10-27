//! A first start with no saved GPU choice, on a real window: the presenter
//! opens on the Vulkan driver of the GPU driving the display alone where the
//! ranking would choose that GPU anyway, and on every driver where it would
//! not.
//!
//! Ignored: it needs a Wayland compositor (and, for the X11 case, an X server
//! with DRI3: an Xwayland under it) and Vulkan drivers. Each case reads
//! `/proc/self/maps` for the drivers this process loaded, so each must run in
//! a process of its own, which nextest gives it. Run on the office laptop
//! (Intel RPL-U on Mesa, NVIDIA MX550 on 615.78) under headless sway, once per
//! GPU — `WLR_RENDER_DRM_DEVICE=/dev/dri/renderD128` for Intel and
//! `renderD129` for NVIDIA — with none of the loader's driver variables set:
//!
//! ```text
//! cargo nextest run -p crcbl-app --test display_driver --run-ignored all
//! ```

#![cfg(target_os = "linux")]

use crcbl_app::adapter::PowerPreference;
use crcbl_app::present::{Presenter, PresenterDesc};
use crcbl_app::window::wait_for_configure;
use crcbl_hal::{DeviceType, SurfaceTarget};
use crcbl_shell::{LogicalSize, Shell, ShellBackend, WindowDesc};
use crcbl_vk::drivers::{Driver, PCI_VENDOR_NVIDIA};

/// A window's presenter and the shell it is on, in the order they drop: the
/// presenter's surface is on the shell's connection, so a shell dropped first
/// — by a failed assertion's unwinding too — leaves the driver tearing down
/// on a dead connection.
struct Opened {
    presenter: Presenter,
    /// The DRM device the compositor names for the window.
    device: u64,
    _shell: Box<dyn Shell>,
}

/// A window on `backend`'s display server, its presenter opened preferring
/// `preference` with no saved choice.
fn open(backend: ShellBackend, preference: PowerPreference) -> Opened {
    let mut shell = crcbl_shell::open_backend(backend).expect("a display server to open on");
    let window = shell
        .create_window(&WindowDesc {
            title: "crcbl display driver",
            size: LogicalSize::new(320.0, 200.0),
            ..WindowDesc::default()
        })
        .expect("create_window");
    let extent = wait_for_configure(&mut *shell, window, &mut |_| {}).expect("a configure");
    let device = match shell.surface_target(window) {
        Ok(
            SurfaceTarget::Wayland {
                main_device: Some(device),
                ..
            }
            | SurfaceTarget::Xcb {
                main_device: Some(device),
                ..
            },
        ) => device,
        other => panic!("the display server names no DRM device for the window: {other:?}"),
    };
    let presenter = Presenter::open(
        &*shell,
        window,
        extent,
        &PresenterDesc {
            power_preference: preference,
            ..PresenterDesc::default()
        },
    )
    .expect("a presenter");
    Opened {
        presenter,
        device,
        _shell: shell,
    }
}

/// Whether this process has a library whose path holds `part` mapped.
fn mapped(part: &str) -> bool {
    std::fs::read_to_string("/proc/self/maps")
        .expect("procfs")
        .lines()
        .any(|line| line.contains(part))
}

/// **A program preferring low power opens on the display's driver alone**,
/// and saves it: the choice it hands back already names the driver, whose
/// library serves the display's vendor, and no other vendor's driver was
/// loaded — on the laptop, NVIDIA's never is while Intel drives the display.
#[test]
#[ignore = "needs a Wayland compositor and Vulkan drivers"]
fn a_first_start_opens_on_the_displays_driver_alone() {
    opens_on_the_displays_driver_alone(ShellBackend::Wayland);
}

/// **The same through X11**: the X server's DRI3 device names the display,
/// here an Xwayland under the compositor.
#[test]
#[ignore = "needs an X server with DRI3 and Vulkan drivers"]
fn a_first_start_on_x11_opens_on_the_displays_driver_alone() {
    opens_on_the_displays_driver_alone(ShellBackend::X11);
}

fn opens_on_the_displays_driver_alone(backend: ShellBackend) {
    let Opened {
        presenter, device, ..
    } = &open(backend, PowerPreference::LowPower);
    let device = *device;
    let opened = presenter.adapter().expect("an adapter");
    let vendor = crcbl_vk::drivers::drm_vendor(device).expect("sysfs names the display's vendor");
    assert_eq!(
        opened.vendor_id, vendor,
        "not the display's GPU: {opened:?}"
    );
    let manifest = presenter
        .gpu_choice()
        .vulkan_driver()
        .expect("opened on the display's driver alone");
    let driver = Driver::read(manifest).expect("the manifest reads");
    eprintln!(
        "opened {:?} through {} ({})",
        opened.name,
        manifest.display(),
        driver.library.display()
    );
    assert!(driver.vendor_hint().is_none_or(|hint| hint == vendor));
    if vendor != PCI_VENDOR_NVIDIA {
        assert!(!mapped("nvidia"), "NVIDIA's driver was loaded");
    }
}

/// **A game on a display an integrated GPU drives opens on every driver**:
/// preferring high performance, it wants a discrete GPU the display's driver
/// alone cannot show it, so its choice names no driver. On a display a
/// discrete GPU drives, that GPU ranks first whatever the preference, and the
/// open stays on its driver.
#[test]
#[ignore = "needs a Wayland compositor and Vulkan drivers"]
fn a_game_on_an_integrated_display_opens_on_every_driver() {
    let Opened {
        presenter, device, ..
    } = &open(ShellBackend::Wayland, PowerPreference::HighPerformance);
    let device = *device;
    let vendor = crcbl_vk::drivers::drm_vendor(device).expect("sysfs names the display's vendor");
    let display_class = {
        let driver = crcbl_vk::drivers::for_vendor(vendor)
            .into_iter()
            .find_map(|driver| {
                crcbl_vk::VkInstance::open_driver(&driver)
                    .ok()?
                    .drm_adapter(device)
            })
            .expect("a driver shows the display's adapter");
        driver.device_type
    };
    let driver = presenter.gpu_choice().vulkan_driver();
    eprintln!("display {display_class:?}, opened through {driver:?}");
    match display_class {
        DeviceType::Integrated => assert_eq!(driver, None, "opened on the iGPU's driver alone"),
        DeviceType::Discrete => assert!(
            driver.is_some(),
            "the display's dGPU opened on every driver"
        ),
        other => panic!("no class to judge by: {other:?}"),
    }
}
