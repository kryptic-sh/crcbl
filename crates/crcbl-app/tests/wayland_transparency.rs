//! S16 through a real compositor: a transparent window's pixels, read back
//! from the screen rather than from the swapchain.
//!
//! An opaque red bar fills the output on the background layer, a transparent
//! window clears itself to blue at half alpha above it, and `grim` reads the
//! pixel at the output's centre, where sway puts the floating window. The
//! three ways a compositor could take the window's alpha give three different
//! pixels, so the assertion names which one happened:
//!
//! | The compositor read the buffer as | Red | Blue |
//! | --- | --- | --- |
//! | premultiplied (what was asked) | half | the stored blue |
//! | straight alpha | half | half the stored blue |
//! | opaque | none | the stored blue |
//!
//! Ignored: it needs sway, whose config floats the `sh.kryptic.crcbl.e2e` app
//! id (`crates/crcbl-shell/tests/wayland-e2e-sway.conf`), `grim`, and a Vulkan
//! driver that presents premultiplied to a Wayland surface. Run it inside
//! `crates/crcbl-shell/tests/sway-session.sh`'s session. Verified on the office
//! laptop's Intel RPL-U under headless sway 1.12 rendering on that GPU
//! (`WLR_RENDER_DRM_DEVICE`), 2026-10-20.
//!
//! The second test is the same window on X11, through sway's Xwayland; the
//! harness passes its `DISPLAY`. Verified there on 2026-10-23 with sway on the
//! Intel GPU, and with sway in software on llvmpipe, CI's setup. **It cannot
//! pass with sway on the laptop's NVIDIA MX550** (driver 615.78), and that is
//! the platform, not the backend: NVIDIA's surface for an Xwayland window
//! offers no premultiplied alpha, and the Intel adapter's swapchain for an X
//! server rendering on NVIDIA fails (`ERROR_INITIALIZATION_FAILED`), so the
//! presenter refuses the window.
//!
//! The third is a plain X server with a compositing manager: Xvfb with picom
//! (`picom --backend xrender`), both windows on X, the screen read with
//! ImageMagick's `import`. Verified on the office laptop on 2026-10-24 with
//! lavapipe pinned (`VK_DRIVER_FILES=…/lvp_icd.json`): without the pin the
//! backdrop opens on the NVIDIA MX550, whose surface for an Xvfb window offers
//! only opaque alpha, and the Intel adapter cannot present to an X server
//! without DRI3. It fails without picom, at the capability check.
//! `crates/crcbl-shell/tests/run-x11-e2e.sh` runs it in its pass with no window
//! manager, starting picom and pinning lavapipe itself.

#![cfg(target_os = "linux")]

use std::time::{Duration, Instant};

use crcbl_app::backend::GpuBackend;
use crcbl_app::present::{Presenter, PresenterDesc};
use crcbl_app::window::wait_for_configure;
use crcbl_hal::{
    Barriers, ClearValue, ColorAttachment, CommandEncoderDesc, CompositeAlpha, ImageBarrier,
    ImageSubresourceRange, LoadOp, Rect2d, RenderPassDesc, ResourceState, StoreOp,
};
use crcbl_shell::{
    LogicalSize, PanelAnchor, PanelDesc, PanelLayer, Shell, ShellBackend, ShellCaps, WindowDesc,
    WindowId, WindowRole,
};

/// The app id the e2e sway config floats, so the window sits at the centre.
const APP_ID: &str = "sh.kryptic.crcbl.e2e";
/// Where sway's floating window is, in the first headless output's
/// 1920x1080.
const SWAY_PROBE: (u32, u32) = (960, 540);
/// Inside both windows on a plain X server with no window manager, where
/// every window opens at the screen's origin.
const X_PROBE: (u32, u32) = (200, 150);
/// The window's colour, premultiplied: blue at half alpha.
const WINDOW: [f32; 4] = [0.0, 0.0, 0.5, 0.5];
/// How far a channel may sit from the expected value: the compositor's blend
/// and the sRGB encode each round once.
const TOLERANCE: i32 = 4;

/// One window and the presenter drawing it a single colour.
struct Solid {
    window: WindowId,
    presenter: Presenter,
    colour: [f32; 4],
}

impl Solid {
    /// A window with a presenter of its own, or one on `beside`'s device.
    fn open(
        shell: &mut dyn Shell,
        desc: &WindowDesc<'_>,
        colour: [f32; 4],
        beside: Option<&Self>,
    ) -> Self {
        let window = shell.create_window(desc).expect("create_window");
        let extent = wait_for_configure(shell, window, &mut |_| {}).expect("a configure");
        let presenter_desc = PresenterDesc {
            label: desc.title,
            backend: Some(GpuBackend::Vulkan),
            ..PresenterDesc::default()
        };
        let presenter = match beside {
            Some(other) => other
                .presenter
                .shared()
                .open(shell, window, extent, &presenter_desc)
                .expect("a presenter on the shared device"),
            None => {
                Presenter::open(shell, window, extent, &presenter_desc).expect("a Vulkan presenter")
            }
        };
        Self {
            window,
            presenter,
            colour,
        }
    }

    /// Clears the next image to the colour and presents it.
    fn present(&mut self) {
        let Some(frame) = self.presenter.acquire().expect("acquire") else {
            return;
        };
        let format = self.presenter.format();
        let device = self.presenter.device();
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("solid"),
            queue: self.presenter.queue(),
        });
        let barrier = |from, to| {
            [ImageBarrier::new(
                frame.image,
                ImageSubresourceRange::all(format),
                from,
                to,
            )]
        };
        encoder.pipeline_barrier(&Barriers {
            images: &barrier(ResourceState::Undefined, ResourceState::ColorAttachment),
            ..Barriers::default()
        });
        encoder.begin_render_pass(&RenderPassDesc {
            label: Some("solid"),
            color_attachments: &[ColorAttachment {
                view: frame.view,
                resolve: None,
                load: LoadOp::Clear,
                store: StoreOp::Store,
                clear: ClearValue::color(self.colour),
            }],
            depth_stencil_attachment: None,
            render_area: Rect2d::from_size(frame.extent.0, frame.extent.1),
            timestamp_writes: None,
        });
        encoder.end_render_pass();
        encoder.pipeline_barrier(&Barriers {
            images: &barrier(ResourceState::ColorAttachment, ResourceState::Present),
            ..Barriers::default()
        });
        let commands = encoder.finish().expect("finish");
        self.presenter
            .submit_and_present(&frame, commands)
            .expect("present");
    }

    fn close(self, shell: &mut dyn Shell) {
        let mut presenter = self.presenter;
        presenter.drain().expect("drain");
        presenter.destroy().expect("destroy");
        shell.destroy_window(self.window).expect("destroy_window");
    }
}

/// The pixel at `at`, read through a Wayland compositor by `grim`.
fn grim_pixel(at: (u32, u32)) -> [u8; 3] {
    let geometry = format!("{},{} 1x1", at.0, at.1);
    let output = crcbl_spawn::command("grim")
        .args(["-t", "ppm", "-g", &geometry, "-"])
        .output()
        .expect("grim is installed");
    assert!(output.status.success(), "grim failed: {output:?}");
    ppm_pixel(&output.stdout)
}

/// The pixel at `at` on `DISPLAY`'s root window, read by ImageMagick's
/// `import`: what the compositing manager drew there, since the root's
/// image includes the overlay window it draws on.
fn import_pixel(at: (u32, u32)) -> [u8; 3] {
    let crop = format!("1x1+{}+{}", at.0, at.1);
    let output = crcbl_spawn::command("import")
        .args(["-window", "root", "-crop", &crop, "-depth", "8", "ppm:-"])
        .output()
        .expect("ImageMagick's import is installed");
    assert!(output.status.success(), "import failed: {output:?}");
    ppm_pixel(&output.stdout)
}

/// The first pixel of a binary PPM.
fn ppm_pixel(bytes: &[u8]) -> [u8; 3] {
    // Three whitespace-separated header fields after the magic, then the
    // pixels.
    let mut fields = 0;
    let mut at = 0;
    while fields < 4 {
        while bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        while !bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        fields += 1;
    }
    let pixel = &bytes[at + 1..at + 4];
    [pixel[0], pixel[1], pixel[2]]
}

/// `value` in 0..=1 as the byte an 8-bit image stores, encoded if `srgb`.
fn stored(value: f32, srgb: bool) -> i32 {
    let encoded = if !srgb {
        value
    } else if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0).round() as i32
}

/// How a test reads the screen back, and where on it the glass is.
#[derive(Clone, Copy)]
struct Readback {
    probe: (u32, u32),
    pixel: fn((u32, u32)) -> [u8; 3],
}

/// Through sway, with `grim`.
const SWAY: Readback = Readback {
    probe: SWAY_PROBE,
    pixel: grim_pixel,
};

/// A red background panel filling sway's output.
fn sway_backdrop() -> WindowDesc<'static> {
    WindowDesc {
        title: "crcbl s16 backdrop",
        app_id: APP_ID,
        size: LogicalSize::new(0.0, 0.0),
        role: WindowRole::Panel(PanelDesc {
            namespace: "crcbl-s16-backdrop",
            layer: PanelLayer::Background,
            anchor: PanelAnchor::TOP | PanelAnchor::BOTTOM | PanelAnchor::LEFT | PanelAnchor::RIGHT,
            ..PanelDesc::default()
        }),
        ..WindowDesc::default()
    }
}

/// The red `backdrop` on `shell`, the half-alpha blue window on
/// `glass_shell` — the same shell, or an X11 one on sway's Xwayland — and the
/// pixel `screen` reads where the window is, which must be the premultiplied
/// blend.
fn assert_blended(
    shell: &mut dyn Shell,
    mut glass_shell: Option<&mut dyn Shell>,
    backdrop: &WindowDesc<'_>,
    screen: Readback,
) {
    let mut backdrop = Solid::open(shell, backdrop, [1.0, 0.0, 0.0, 1.0], None);
    let glass_desc = WindowDesc {
        title: "crcbl s16 glass",
        app_id: APP_ID,
        size: LogicalSize::new(400.0, 300.0),
        transparent: true,
        ..WindowDesc::default()
    };
    // A Wayland glass is on the backdrop's device: two windows each with a
    // device of their own crash NVIDIA's driver in the second one's teardown
    // (see `crcbl_app::present::SharedGpu`), and one device is how a program
    // with two windows should run anyway. An X11 glass picks its own adapter:
    // NVIDIA's surface for an Xwayland window offers no premultiplied alpha
    // (the office laptop's MX550, driver 615.78, where sway renders on it),
    // so the backdrop's device may not present it, and `Presenter::open`
    // passes over an adapter that cannot.
    let mut glass = match glass_shell.as_deref_mut() {
        Some(x11) => Solid::open(x11, &glass_desc, WINDOW, None),
        None => Solid::open(shell, &glass_desc, WINDOW, Some(&backdrop)),
    };
    assert_eq!(
        glass.presenter.composite_alpha(),
        CompositeAlpha::PreMultiplied,
        "the presenter asked the swapchain for premultiplied alpha"
    );
    let srgb = glass.presenter.format().is_srgb();
    let blue = stored(WINDOW[2], srgb);
    let half = 128;

    // Both keep presenting until the screen shows the window over the bar, or
    // the deadline names what it showed instead. Each shell's events go to
    // the presenter of its own window: two shells number windows apart, so
    // one's id can be the other's.
    let deadline = Instant::now() + Duration::from_secs(10);
    let pixel = loop {
        match glass_shell.as_deref_mut() {
            Some(x11) => {
                shell.pump(&mut |event| backdrop.presenter.observe(&event));
                x11.pump(&mut |event| glass.presenter.observe(&event));
            }
            None => shell.pump(&mut |event| {
                backdrop.presenter.observe(&event);
                glass.presenter.observe(&event);
            }),
        }
        backdrop.present();
        glass.present();
        std::thread::sleep(Duration::from_millis(50));
        let pixel = (screen.pixel)(screen.probe);
        if i32::from(pixel[2]) > 0 || Instant::now() > deadline {
            break pixel;
        }
    };
    println!(
        "crcbl s16: the screen shows {pixel:?} at {:?} (stored blue {blue}) on {:?}",
        screen.probe,
        glass.presenter.adapter().map(|adapter| adapter.name)
    );
    let [red, green, observed_blue] = pixel.map(i32::from);
    let near = |value: i32, want: i32| (value - want).abs() <= TOLERANCE;
    assert!(
        near(red, half) && near(green, 0) && near(observed_blue, blue),
        "expected premultiplied ({half}, 0, {blue}); straight alpha would be \
         ({half}, 0, {}), opaque (0, 0, {blue}); got {pixel:?}",
        blue / 2
    );

    match glass_shell {
        Some(x11) => glass.close(x11),
        None => glass.close(shell),
    }
    backdrop.close(shell);
}

#[test]
#[ignore = "needs headless sway, grim and a Vulkan driver; see the module docs"]
fn a_transparent_window_is_blended_premultiplied_by_the_compositor() {
    let mut shell = crcbl_shell::open_backend(ShellBackend::Wayland).expect("a Wayland session");
    assert_blended(shell.as_mut(), None, &sway_backdrop(), SWAY);
}

/// **The same through X11**: the window on sway's Xwayland, created on the
/// 32-bit visual, which Xwayland hands sway with its alpha.
#[test]
#[ignore = "needs headless sway with Xwayland, DISPLAY naming it, grim and a Vulkan driver"]
fn an_xwayland_window_is_blended_premultiplied_too() {
    let mut shell = crcbl_shell::open_backend(ShellBackend::Wayland).expect("a Wayland session");
    let mut x11 = crcbl_shell::open_backend(ShellBackend::X11).expect("sway's Xwayland");
    assert!(
        x11.caps().contains(ShellCaps::TRANSPARENT),
        "Xwayland composites, so the X11 backend offers transparent windows"
    );
    assert_blended(shell.as_mut(), Some(x11.as_mut()), &sway_backdrop(), SWAY);
}

/// **The same on a plain X server** whose compositing manager owns
/// `_NET_WM_CM_S0` (picom on Xvfb, say): the 32-bit visual is offered
/// because something composites, and the manager blends the window by its
/// alpha. Both windows are on the X server, with no window manager, so each
/// opens at the screen's origin, the glass above the opaque red one.
#[test]
#[ignore = "needs an X server with a compositing manager, DISPLAY naming it, ImageMagick and a Vulkan driver"]
fn a_window_under_an_x_compositing_manager_is_blended_premultiplied() {
    let mut x11 = crcbl_shell::open_backend(ShellBackend::X11).expect("an X server");
    assert!(
        x11.caps().contains(ShellCaps::TRANSPARENT),
        "a compositing manager owns _NET_WM_CM_S0, so the X11 backend offers \
         transparent windows"
    );
    let backdrop = WindowDesc {
        title: "crcbl s16 backdrop",
        app_id: APP_ID,
        size: LogicalSize::new(800.0, 600.0),
        ..WindowDesc::default()
    };
    let screen = Readback {
        probe: X_PROBE,
        pixel: import_pixel,
    };
    assert_blended(x11.as_mut(), None, &backdrop, screen);
}
