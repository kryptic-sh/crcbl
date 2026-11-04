//! The shared-image half of presenting another API's frames, on WARP.
//!
//! The renderer here is a second D3D11 device standing in for Vulkan: it opens
//! the images and both fences through the NT handles [`Interop`] shares, the
//! way `crcbl-vk`'s `win32_interop` imports them, so what is checked is the
//! sharing itself — handles, cross-device fences and the copy — on the
//! software adapter every Windows machine has. No Vulkan driver a CI runner can
//! install lists the Win32 interop extensions with Vulkan 1.3, so this is the
//! half of the path CI can drive; `docs/backlog.md` (_Vulkan on a transparent
//! Win32 window_) records what is left.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crcbl_hal::{CompositeAlpha, Format, HalError};
use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11DeviceContext4, ID3D11Fence, ID3D11RenderTargetView, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory2, DXGI_CREATE_FACTORY_FLAGS, IDXGIAdapter1, IDXGIFactory4,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, SW_SHOWNOACTIVATE, ShowWindow, WS_EX_NOREDIRECTIONBITMAP,
    WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::core::PCWSTR;

use super::{Interop, SharedPresenter, open_device};
use crate::Composition;

/// The shared images' extent: 64 texels of four bytes is one 256-byte row, a
/// pitch every driver lays out without padding.
const EXTENT: (u32, u32) = (64, 4);

/// Two clear colours, and the bytes each lands as in a `Bgra8Unorm` texture.
/// Every channel is a multiple of 17, so `f32` to `unorm8` is exact.
const FIRST: ([f32; 4], [u8; 4]) = (
    [34.0 / 255.0, 68.0 / 255.0, 102.0 / 255.0, 136.0 / 255.0],
    [0x66, 0x44, 0x22, 0x88],
);
const SECOND: ([f32; 4], [u8; 4]) = (
    [17.0 / 255.0, 51.0 / 255.0, 85.0 / 255.0, 1.0],
    [0x55, 0x33, 0x11, 0xFF],
);

/// How long a fence is waited for before the test calls it lost.
const FENCE_DEADLINE: Duration = Duration::from_secs(10);

/// The LUID of WARP, the software adapter, as [`Interop::open`] takes one.
fn warp_luid() -> [u8; 8] {
    // SAFETY: no flags; the factory and adapter are released at the end of
    // this call, and `GetDesc1` only fills the returned struct.
    let luid = unsafe {
        let factory: IDXGIFactory4 =
            CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)).expect("a DXGI factory");
        let adapter: IDXGIAdapter1 = factory.EnumWarpAdapter().expect("WARP ships with Windows");
        adapter
            .GetDesc1()
            .expect("WARP describes itself")
            .AdapterLuid
    };
    let mut bytes = [0; 8];
    bytes[..4].copy_from_slice(&luid.LowPart.to_le_bytes());
    bytes[4..].copy_from_slice(&luid.HighPart.to_le_bytes());
    bytes
}

/// The renderer's side: a device of its own with every shared image and both
/// fences opened from their handles.
struct Renderer {
    images: Vec<ID3D11RenderTargetView>,
    ready: ID3D11Fence,
    done: ID3D11Fence,
    context: ID3D11DeviceContext4,
}

impl Renderer {
    /// Opens what `interop` shares on a device of its own on `luid`.
    fn open(luid: [u8; 8], interop: &Interop) -> Self {
        let (device, context) = open_device(luid).expect("a second D3D11 device");
        let handle = |raw: isize| HANDLE(raw as *mut core::ffi::c_void);
        // SAFETY: every handle is a live NT handle `interop` shared and keeps
        // open; each open takes the device's own reference to the object.
        unsafe {
            let images = (0..interop.image_count())
                .map(|index| {
                    let texture: ID3D11Texture2D = device
                        .OpenSharedResource1(handle(interop.image_handle(index)))
                        .expect("the shared texture opens");
                    let mut view = None;
                    device
                        .CreateRenderTargetView(&texture, None, Some(&raw mut view))
                        .expect("a render target view of it");
                    view.expect("a view")
                })
                .collect();
            let fence = |raw: isize| {
                let mut fence: Option<ID3D11Fence> = None;
                device
                    .OpenSharedFence(handle(raw), &raw mut fence)
                    .expect("the shared fence opens");
                fence.expect("a fence")
            };
            Self {
                images,
                ready: fence(interop.ready_handle()),
                done: fence(interop.done_handle()),
                context,
            }
        }
    }

    /// Clears image `index` to `colour`, then signals `ready` with `value`.
    fn draw(&self, index: u32, colour: [f32; 4], value: u64) {
        // SAFETY: the view and fence are this device's, used from one thread.
        unsafe {
            self.context
                .ClearRenderTargetView(&self.images[index as usize], &colour);
            self.context
                .Signal(&self.ready, value)
                .expect("ready is signalled");
            // Without it the clear and the signal could sit in this context
            // while the other device waits for them.
            self.context.Flush();
        }
    }

    /// Waits until `done` has reached `value`: the copy out of an image ran.
    fn wait_done(&self, value: u64) {
        let start = Instant::now();
        // SAFETY: the fence is live; reading its value has no other effect.
        while unsafe { self.done.GetCompletedValue() } < value {
            assert!(
                start.elapsed() < FENCE_DEADLINE,
                "done never reached {value}: the copy did not signal it"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

/// Whether every texel of `pixels` is `texel`, with what the first was.
fn assert_every_texel(pixels: &[u8], texel: [u8; 4]) {
    assert_eq!(pixels.len(), (EXTENT.0 * EXTENT.1 * 4) as usize);
    assert!(
        pixels.as_chunks::<4>().0.iter().all(|read| *read == texel),
        "read {:?} where the renderer cleared {texel:?}",
        &pixels[..4]
    );
}

/// **Each image another device draws reaches [`Interop::read_image`] whole,
/// by its own index, and `done` is signalled behind every copy.**
///
/// Red if two indices share a texture or a handle names the wrong one (the
/// colours cross), if a fence handle opens another fence (the wait for
/// `ready` or `done` never ends), or if `done` is not signalled per copy.
///
/// Each image is drawn before it is read, never raced: on WARP, a read queued
/// ahead of its `ready` signal stalled the renderer's device too, and the test
/// hung (seen 2026-11-05). So whether the copy waits for `ready` is left to
/// `crcbl-vk`'s `a_shared_texture_cleared_by_vulkan_reads_back_in_d3d11`,
/// where the read is issued while Vulkan's clear may still be running.
#[test]
fn a_renderer_s_images_are_read_back_by_index() {
    let luid = warp_luid();
    let mut interop = Interop::open(luid, EXTENT, Format::Bgra8Unorm, 2).expect("an interop");
    let renderer = Renderer::open(luid, &interop);

    renderer.draw(1, FIRST.0, 1);
    assert_every_texel(&interop.read_image(1, 1).expect("image 1"), FIRST.1);
    renderer.draw(0, SECOND.0, 2);
    assert_every_texel(&interop.read_image(0, 2).expect("image 0"), SECOND.1);
    assert_every_texel(&interop.read_image(1, 2).expect("image 1 again"), FIRST.1);
    // One copy each, so three `done` signals.
    renderer.wait_done(3);
}

/// **A format or shape the ring cannot have is refused by name**, before any
/// device is made.
#[test]
fn an_interop_refuses_a_format_it_cannot_present_and_an_empty_ring() {
    let luid = [0; 8];
    for (extent, format, count) in [
        (EXTENT, Format::D32Float, 2),
        ((0, 4), Format::Bgra8Unorm, 2),
        (EXTENT, Format::Bgra8Unorm, 0),
    ] {
        let refused = Interop::open(luid, extent, format, count)
            .expect_err("refused before the LUID is looked up");
        assert!(
            matches!(refused, HalError::InvalidDescriptor(_)),
            "{extent:?} {format:?} x {count}: {refused:?}"
        );
    }
}

/// The environment variables a windowed test runs under: CI sets the first,
/// and a developer who wants the window on their own screen sets the second.
const DESKTOP_ALLOWED: [&str; 2] = ["CI", "CRCBL_DESKTOP_TESTS"];

/// A transparent window — `WS_EX_NOREDIRECTIONBITMAP`, as the Win32 shell
/// makes one — for the length of a test, shown without focus and destroyed on
/// drop. `crcbl-dx12`'s swapchain tests carry the same window and argue its
/// choices.
struct TestWindow(HWND);

impl TestWindow {
    fn open(size: (u32, u32)) -> Self {
        assert!(
            DESKTOP_ALLOWED
                .iter()
                .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty())),
            "this test shows a window on the real desktop, so it runs only where one of \
             {DESKTOP_ALLOWED:?} is set"
        );
        let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
        let title: Vec<u16> = "crcbl-dcomp shared presenter\0".encode_utf16().collect();
        let width = i32::try_from(size.0).expect("a test window fits in an i32");
        let height = i32::try_from(size.1).expect("a test window fits in an i32");
        // SAFETY: both strings are NUL-terminated UTF-16 that outlive the call,
        // and `"STATIC"` is a class user32 registers in every process, so no
        // module handle is needed to name it.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOREDIRECTIONBITMAP,
                PCWSTR::from_raw(class.as_ptr()),
                PCWSTR::from_raw(title.as_ptr()),
                WS_POPUP,
                0,
                0,
                width,
                height,
                None,
                None,
                None,
                None,
            )
        }
        .expect("a runner with a desktop creates a window");
        // SAFETY: `hwnd` was just created. The result says whether it was
        // visible before, which is not a failure.
        let _ = unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
        Self(hwnd)
    }
}

impl Drop for TestWindow {
    fn drop(&mut self) {
        // SAFETY: this object's own window, created on this thread.
        if let Err(error) = unsafe { DestroyWindow(self.0) } {
            crcbl_core::log::debug!("crcbl-dcomp: a test window would not close: {error}");
        }
    }
}

/// Draws and presents `frames` frames, the renderer reusing an image only once
/// the copy out of it signalled `done`, and returns the last `done` value.
///
/// `ready` and `done` both continue from `from`, the value each reached
/// before: one `ready` signal per frame, one `done` signal per copy.
fn present_frames(
    presenter: &mut SharedPresenter,
    renderer: &Renderer,
    frames: u64,
    from: u64,
) -> u64 {
    let count = u64::from(presenter.interop().image_count());
    let mut done_for = vec![0; count as usize];
    let mut done = from;
    for frame in from + 1..=from + frames {
        let index = ((frame - 1) % count) as u32;
        renderer.wait_done(done_for[index as usize]);
        let colour = if frame % 2 == 0 { FIRST.0 } else { SECOND.0 };
        renderer.draw(index, colour, frame);
        done = presenter.present(index, frame, 0).expect("a present");
        assert_eq!(done, frame, "one done signal per present");
        done_for[index as usize] = done;
    }
    renderer.wait_done(done);
    done
}

/// **A shared presenter on a transparent window binds, presents a renderer's
/// frames, resizes, and gives the visual back when it goes.**
///
/// What a green run proves, beyond what the headless tests here do:
///
/// * `CreateSwapChainForComposition` takes the presenter's descriptor on the
///   interop's own device, and the window's visual takes it as content.
/// * Presenting copies behind each `ready` value and signals one `done` value
///   per frame, which the renderer can wait for to reuse an image.
/// * `reconfigure` rebuilds the ring at a new size, count and format and
///   resizes the swapchain, and presenting carries on from there.
/// * An alpha mode a composition swapchain cannot take and a second presenter
///   on the bound window are refused by name, and a presenter made after the
///   first is dropped binds.
///
/// What it cannot prove is what DWM shows: nothing reads the desktop back.
#[test]
#[ignore = "shows a window on the desktop; CI's dx12 e2e job runs it"]
fn a_shared_presenter_presents_a_renderer_s_frames_to_a_transparent_window() {
    let luid = warp_luid();
    let window = TestWindow::open(EXTENT);
    let composition = Arc::new(Composition::for_window(window.0).expect("a composition"));
    let open =
        |alpha| SharedPresenter::new(&composition, luid, EXTENT, Format::Bgra8UnormSrgb, 2, alpha);

    let inherit = open(CompositeAlpha::Inherit).expect_err("no inherited alpha");
    assert!(
        matches!(inherit, HalError::InvalidDescriptor(_)),
        "{inherit:?}"
    );
    let mut presenter = open(CompositeAlpha::PreMultiplied).expect("a presenter");
    let second = open(CompositeAlpha::PreMultiplied).expect_err("the visual is taken");
    assert!(
        matches!(second, HalError::InvalidDescriptor(_)),
        "{second:?}"
    );

    let renderer = Renderer::open(luid, presenter.interop());
    let done = present_frames(&mut presenter, &renderer, 4, 0);

    // Every old image is idle on both sides: the renderer waited for the last
    // `done`, which follows every copy.
    drop(renderer);
    presenter
        .reconfigure((32, 8), Format::Rgba8Unorm, 3)
        .expect("a reconfigure");
    assert_eq!(presenter.interop().image_count(), 3);
    let renderer = Renderer::open(luid, presenter.interop());
    present_frames(&mut presenter, &renderer, 5, done);
    let refused = presenter
        .reconfigure(EXTENT, Format::D32Float, 2)
        .expect_err("no depth ring");
    assert!(
        matches!(refused, HalError::InvalidDescriptor(_)),
        "{refused:?}"
    );

    drop(renderer);
    drop(presenter);
    let again = open(CompositeAlpha::Opaque).expect("the first presenter gave the visual back");
    drop(again);
}
