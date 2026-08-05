//! The DXGI half of presentation: the flip-model swapchain, the waitable
//! object, and what a failed present means.
//!
//! [`crate::present`] is the other half and holds every decision that is
//! arithmetic — which formats are offered, how many buffers, what a
//! [`PresentMode`] becomes, whether an id can be waited for. Everything that
//! needs `windows` is here, so the split is also the line between what a Linux
//! `cargo test` executes and what only CI can.
//!
//! # Why the waitable object had to be decided at creation time
//!
//! `DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT` is a **creation** flag:
//! a swapchain made without it has no `GetFrameLatencyWaitableObject` to hand
//! out, and the only way to add one is to build a new swapchain. So a slice
//! that landed the swapchain and left present feedback for later would have
//! built exactly the object the next slice replaced. The same argument covers
//! `DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING`, which is why it is set whenever the
//! factory reports tearing rather than only when the caller asked for
//! [`PresentMode::Immediate`] — a reconfigure into that mode would otherwise
//! need a new swapchain, and `reconfigure_swapchain`'s whole promise is that
//! the handle survives.
//!
//! # Flip-model presents sRGB through the view, not the buffer
//!
//! `DXGI_SWAP_EFFECT_FLIP_DISCARD` rejects an `_SRGB` back-buffer format
//! outright. The sanctioned answer — and the one every D3D12 sample uses — is a
//! linear back buffer with an sRGB render target view over it, which is a cast
//! D3D12 permits on a swapchain buffer without the typeless resource
//! `crate::device`'s `create_image_view` otherwise insists on. [`present::buffer_format`]
//! is the buffer half and the seam format the caller asked for is the view
//! half, and `create_swapchain` builds the views itself rather than through the
//! seam's `create_image_view`, which is where the equal-format rule lives.
//!
//! # DXGI has no `OUT_OF_DATE`, so neither does this backend
//!
//! `vkAcquireNextImageKHR` reports a swapchain that no longer matches its
//! surface, and the seam carries that as [`SurfaceError::OutOfDate`] and
//! `AcquiredFrame::suboptimal`. DXGI reports nothing of the kind: a window
//! resized under a flip-model swapchain simply stretches, and the application
//! learns about the resize from its own message loop. So this backend never
//! produces either, and a caller reconfigures because the shell told it to. That
//! is stated rather than left to be inferred, because a backend that *invented*
//! an `OutOfDate` would put a frame loop into an unending reconfigure.

use core::time::Duration;

use crcbl_hal::{
    Format, HalError, ImageHandle, ImageViewHandle, PresentMode, SurfaceError, SwapchainDesc,
};
use windows::Win32::Foundation::{HANDLE, HWND, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows::Win32::Graphics::Direct3D12::{
    D3D12_REQ_TEXTURE2D_U_OR_V_DIMENSION, ID3D12CommandQueue,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_ALPHA_MODE_IGNORE, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    DXGI_ERROR_DEVICE_REMOVED, DXGI_ERROR_DEVICE_RESET, DXGI_MWA_NO_ALT_ENTER, DXGI_PRESENT,
    DXGI_PRESENT_ALLOW_TEARING, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG,
    DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING, DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT,
    DXGI_SWAP_EFFECT_FLIP_DISCARD, DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIFactory4, IDXGISwapChain3,
};
use windows::Win32::System::Threading::WaitForSingleObjectEx;
use windows::core::{BOOL, Interface};

use crate::conv;
use crate::handle::Owned;
use crate::present::{self, PresentLedger};

/// What a swapchain handle resolves to.
#[derive(Debug)]
pub(crate) struct SwapchainEntry {
    /// Device that created it, per obligation 3.
    pub(crate) owner: u64,
    pub(crate) raw: IDXGISwapChain3,
    /// `GetFrameLatencyWaitableObject`, as a plain address.
    ///
    /// A `HANDLE` is a raw pointer `windows-rs` declares neither `Send` nor
    /// `Sync`, and this entry lives in a pool behind the device's one
    /// [`Mutex`](std::sync::Mutex) inside a struct the seam requires to be
    /// both. Keeping the integer is what leaves this crate with no `unsafe`
    /// marker impl; the handle is rebuilt inside [`wait`].
    ///
    /// **Not closed when the swapchain is destroyed.** Whose handle it is, is
    /// the one thing about this object the documentation is not crisp on, and
    /// the two errors are not symmetric: a double `CloseHandle` is a
    /// process-wide fault, while a handle left open is bounded by the number of
    /// swapchains a process makes and goes at exit. `wgpu-hal` 29's D3D12
    /// backend does not close it either — checked, rather than assumed.
    /// `docs/backlog.md` records it as unsettled.
    pub(crate) waitable: usize,
    /// The window, kept so `reconfigure_swapchain` can tell a resize from an
    /// attempt to move the swapchain to a different surface — and so the entry
    /// survives its surface handle being destroyed, which obligation 2 permits.
    pub(crate) hwnd: usize,
    /// The extent this swapchain was **actually** configured at, reported on
    /// every [`AcquiredFrame::extent`](crcbl_hal::AcquiredFrame::extent) per
    /// the seam's obligation 3.
    pub(crate) extent: (u32, u32),
    /// The seam format the *views* carry. The buffers are
    /// [`present::buffer_format`] of it.
    pub(crate) format: Format,
    pub(crate) buffers: u32,
    /// The mode this swapchain resolved to, which is what
    /// [`present::pacing`] is asked about on every present.
    pub(crate) present_mode: PresentMode,
    /// The creation flags, kept because `ResizeBuffers` takes them again and
    /// must be given the same ones — a resize that dropped the waitable-object
    /// flag would leave `GetFrameLatencyWaitableObject`'s handle signalling for
    /// a swapchain that no longer counts frames.
    pub(crate) flags: DXGI_SWAP_CHAIN_FLAG,
    /// One [`ImageHandle`] per back buffer, reissued on every reconfigure.
    pub(crate) images: Vec<ImageHandle>,
    /// One whole-image render target view per back buffer, owned by the
    /// swapchain and handed out through `AcquiredFrame::view`.
    pub(crate) views: Vec<ImageViewHandle>,
    /// The present ids this object has been given. Replaced wholesale on
    /// reconfigure, which is what restarts the numbering.
    pub(crate) ledger: PresentLedger,
}

impl Owned for SwapchainEntry {
    fn owner(&self) -> u64 {
        self.owner
    }
}

/// A freshly created DXGI swapchain, before the device has given its buffers
/// handles.
#[derive(Debug)]
pub(crate) struct Created {
    pub(crate) raw: IDXGISwapChain3,
    pub(crate) waitable: usize,
    pub(crate) extent: (u32, u32),
    pub(crate) buffers: u32,
    pub(crate) present_mode: PresentMode,
    pub(crate) flags: DXGI_SWAP_CHAIN_FLAG,
}

/// The creation flags for a swapchain on this machine.
///
/// Both are decided once and for the swapchain's whole life; see the module
/// docs for why neither can be added later.
pub(crate) fn flags(tearing: bool) -> DXGI_SWAP_CHAIN_FLAG {
    let mut flags = DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT;
    if tearing {
        flags = DXGI_SWAP_CHAIN_FLAG(flags.0 | DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING.0);
    }
    flags
}

/// Checks a [`SwapchainDesc`] against what a flip-model swapchain accepts, and
/// answers the size and count it will be configured at.
///
/// Split out of [`create`] because it is also what `reconfigure_swapchain`
/// needs, and because every rule here is one D3D12 would otherwise report as a
/// bare `E_INVALIDARG` with no field named.
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`], naming the field and why.
pub(crate) fn check(desc: &SwapchainDesc<'_>) -> Result<((u32, u32), u32), HalError> {
    if desc.extent.0 == 0 || desc.extent.1 == 0 {
        return Err(HalError::InvalidDescriptor(format!(
            "SwapchainDesc::extent is {:?}; an unconfigured or minimized window means \"do not \
             create a swapchain yet\" rather than \"pick something\"",
            desc.extent
        )));
    }
    if !present::is_flip_model_format(desc.format) {
        return Err(HalError::InvalidDescriptor(format!(
            "DXGI_SWAP_EFFECT_FLIP_DISCARD does not accept {:?}; Instance::surface_caps offers \
             {:?}",
            desc.format,
            present::FLIP_MODEL_FORMATS
        )));
    }
    if desc.composite_alpha != crcbl_hal::CompositeAlpha::Opaque {
        return Err(HalError::InvalidDescriptor(format!(
            "CreateSwapChainForHwnd composites an HWND swapchain opaquely, so {:?} is not \
             configurable; Instance::surface_caps offers CompositeAlpha::Opaque alone",
            desc.composite_alpha
        )));
    }

    // The one range the platform genuinely pins, and therefore the one place
    // the seam's obligation 2 ("clamp into the platform's permitted range, and
    // report what was configured") has anything to do on Windows. A window
    // system that reported a size past D3D12's 2D ceiling would otherwise reach
    // `CreateSwapChainForHwnd` as `E_INVALIDARG`.
    let ceiling = D3D12_REQ_TEXTURE2D_U_OR_V_DIMENSION;
    let extent = (desc.extent.0.min(ceiling), desc.extent.1.min(ceiling));
    if extent != desc.extent {
        log::warn!(
            "crcbl-dx12: a swapchain was asked for at {:?} and clamped to {extent:?}, which is \
             D3D12's 2D ceiling; render at AcquiredFrame::extent",
            desc.extent
        );
    }
    Ok((extent, present::buffer_count(desc.image_count)))
}

/// Creates the DXGI swapchain, sets its frame latency, and takes its waitable
/// object.
///
/// `queue` is the device's `ID3D12CommandQueue` and not the `ID3D12Device`:
/// D3D12 is the one API where `CreateSwapChainForHwnd`'s "device" argument is
/// the **queue** presents are ordered against, which is also why a swapchain is
/// device-scoped while the surface it names is not.
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`] as [`check`], or the DXGI failure through
/// [`surface_error`].
pub(crate) fn create(
    factory: &IDXGIFactory4,
    queue: &ID3D12CommandQueue,
    hwnd: HWND,
    desc: &SwapchainDesc<'_>,
    tearing: bool,
) -> Result<Created, SurfaceError> {
    let (extent, buffers) = check(desc)?;
    let present_mode = present::resolve_present_mode(desc.present_mode, tearing);
    let flags = flags(tearing);
    let desc1 = DXGI_SWAP_CHAIN_DESC1 {
        Width: extent.0,
        Height: extent.1,
        // Linear here, sRGB on the view. See the module docs.
        Format: conv::dxgi_format(present::buffer_format(desc.format)),
        Stereo: BOOL::from(false),
        // Flip-model swapchains are single-sampled by definition: multisampling
        // is done on an offscreen target and resolved into the back buffer,
        // which is what the engine's tonemap pass already does.
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: buffers,
        // Stretch rather than `DXGI_SCALING_NONE`, so the frames rendered
        // between a window resize and the swapchain catching up still cover the
        // window instead of leaving an unpainted border.
        Scaling: DXGI_SCALING_STRETCH,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
        AlphaMode: DXGI_ALPHA_MODE_IGNORE,
        Flags: flags.0.cast_unsigned(),
    };

    // SAFETY: `factory` and `queue` are live COM interfaces this crate owns
    // references to; `hwnd` is the address the caller promised names a live
    // window in `Instance::create_surface`'s safety contract. `desc1` is a
    // fully initialised local borrowed for the call. `None` for the fullscreen
    // descriptor means windowed, and `None` for the output restriction means
    // "any" — both are documented as optional.
    let chain = unsafe { factory.CreateSwapChainForHwnd(queue, hwnd, &desc1, None, None) }
        .map_err(|error| surface_error("CreateSwapChainForHwnd", &error))?;
    let raw: IDXGISwapChain3 = chain
        .cast()
        .map_err(|error| surface_error("QueryInterface for IDXGISwapChain3", &error))?;

    let latency = present::frame_latency(buffers);
    // SAFETY: `raw` is the swapchain just created, and it carries
    // `DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT` — which is what
    // makes both of these calls legal on it. The call takes one scalar.
    unsafe { raw.SetMaximumFrameLatency(latency) }
        .map_err(|error| surface_error("SetMaximumFrameLatency", &error))?;
    // SAFETY: as above. The call reads nothing of ours and returns a handle by
    // value; see `SwapchainEntry::waitable` for why it is kept as an integer
    // and not closed.
    let waitable = unsafe { raw.GetFrameLatencyWaitableObject() }.0 as usize;

    // Alt+Enter otherwise puts DXGI's own message hook in charge of a
    // fullscreen transition the engine did not ask for and cannot see, which
    // leaves the swapchain in a mode nothing above the seam has vocabulary for.
    // A failure is logged rather than propagated: it costs a key binding, not a
    // swapchain.
    //
    // SAFETY: `factory` is live and `hwnd` is the caller's window, as above.
    if let Err(error) = unsafe { factory.MakeWindowAssociation(hwnd, DXGI_MWA_NO_ALT_ENTER) } {
        log::debug!("crcbl-dx12: DXGI kept its Alt+Enter handling on this window: {error}");
    }

    log::debug!(
        "crcbl-dx12: created a {buffers}-buffer {extent:?} {:?} swapchain, {present_mode:?}, \
         latency {latency}, tearing={tearing}",
        desc.format
    );
    Ok(Created {
        raw,
        waitable,
        extent,
        buffers,
        present_mode,
        flags,
    })
}

/// `ResizeBuffers` for a reconfigure, with the same flags the swapchain was
/// created with.
///
/// Every reference to the old back buffers must already be gone — the caller's
/// job, and `crate::device`'s `reconfigure_swapchain` does it by destroying the
/// views and images first and waiting for the queue.
///
/// # Errors
///
/// As [`surface_error`].
pub(crate) fn resize(
    entry: &SwapchainEntry,
    extent: (u32, u32),
    buffers: u32,
) -> Result<(), SurfaceError> {
    // SAFETY: `entry.raw` is a live swapchain this device created. The format
    // and flags are the ones it was created with, which is what `ResizeBuffers`
    // requires of a swapchain that carries the waitable-object flag.
    unsafe {
        entry.raw.ResizeBuffers(
            buffers,
            extent.0,
            extent.1,
            conv::dxgi_format(present::buffer_format(entry.format)),
            entry.flags,
        )
    }
    .map_err(|error| surface_error("ResizeBuffers", &error))
}

/// Presents a swapchain's current back buffer.
///
/// Takes the interface by reference rather than the whole entry, because the
/// caller has to have released the device lock before calling it: `Present`
/// blocks when the frame queue is full, and holding the device's one
/// [`Mutex`](std::sync::Mutex) across a vblank would stall every other thread's
/// `create_buffer` for a frame.
///
/// # Errors
///
/// As [`surface_error`]. `DXGI_STATUS_OCCLUDED` — the window is fully hidden —
/// is a **success** code and is treated as one: the frame was accepted, the
/// compositor simply will not show it, and turning that into an error would
/// fail every frame of a minimized window.
pub(crate) fn present(raw: &IDXGISwapChain3, mode: PresentMode) -> Result<(), SurfaceError> {
    let pacing = present::pacing(mode);
    let flags = if pacing.allow_tearing {
        DXGI_PRESENT_ALLOW_TEARING
    } else {
        DXGI_PRESENT::default()
    };
    // SAFETY: `raw` is a live swapchain this device created and holds a
    // reference to for the duration of this call. Both arguments are scalars,
    // and `present::pacing` is what guarantees the tearing flag never arrives
    // beside a non-zero sync interval — which DXGI rejects.
    let result = unsafe { raw.Present(pacing.sync_interval, flags) };
    result
        .ok()
        .map_err(|error| surface_error("IDXGISwapChain::Present", &error))
}

/// Blocks on a frame-latency waitable object.
///
/// A `waitable` of zero is "there is nothing to wait on" and returns at once;
/// every swapchain this backend creates has one, so it is the shape of the
/// answer rather than a case that arises.
///
/// # Errors
///
/// [`SurfaceError::Timeout`] when the timeout lapsed with the swapchain still
/// full — the seam calls that expected traffic on a stalled compositor.
/// [`HalError::Backend`] for a wait that failed outright, which on this call
/// means the handle is not one.
pub(crate) fn wait(waitable: usize, timeout: Duration) -> Result<(), SurfaceError> {
    if waitable == 0 {
        return Ok(());
    }
    let handle = HANDLE(core::ptr::without_provenance_mut(waitable));
    // SAFETY: `handle` is the waitable object of a live swapchain this device
    // created and holds a reference to for the duration of this call — the
    // device lock was released before it, but the entry cannot be removed
    // without the swapchain handle, which the caller holds. `false` for
    // alertable, so no APC can make this return early with a value neither arm
    // below expects.
    let waited = unsafe { WaitForSingleObjectEx(handle, present::timeout_millis(timeout), false) };
    match waited {
        WAIT_OBJECT_0 => Ok(()),
        WAIT_TIMEOUT => Err(SurfaceError::Timeout),
        other => Err(SurfaceError::Hal(HalError::Backend(format!(
            "waiting on a frame-latency waitable object returned {other:?}"
        )))),
    }
}

/// A DXGI failure as the seam spells it.
///
/// Two codes are separated out and everything else keeps the driver's message:
///
/// * `DXGI_ERROR_DEVICE_REMOVED` and `DXGI_ERROR_DEVICE_RESET` are
///   [`HalError::DeviceLost`] — the adapter is gone or was reset, which no
///   reconfigure recovers from and which the caller has to rebuild everything
///   for.
/// * Everything else is [`HalError::Backend`] with the call named.
///
/// **Nothing maps to [`SurfaceError::OutOfDate`]**, and that is deliberate:
/// DXGI never reports it (see the module docs), so a code translated into it
/// would put a caller into an unending reconfigure loop over a condition
/// reconfiguring cannot fix.
pub(crate) fn surface_error(what: &str, error: &windows::core::Error) -> SurfaceError {
    let code = error.code();
    if code == DXGI_ERROR_DEVICE_REMOVED || code == DXGI_ERROR_DEVICE_RESET {
        return SurfaceError::Hal(HalError::DeviceLost(format!("{what} failed: {error}")));
    }
    SurfaceError::Hal(HalError::Backend(format!("{what} failed: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr::NonNull;
    use std::time::Instant;

    use crcbl_core::SurfaceTarget;
    use crcbl_hal::{
        Barriers, ClearValue, ColorAttachment, CommandEncoderDesc, CompositeAlpha, Device as _,
        ImageBarrier, ImageSubresourceRange, Instance as _, LoadOp, PresentInfo, QueueKind, Rect2d,
        RenderPassDesc, ResourceState, StoreOp, SubmitInfo, SurfaceCaps, SurfaceHandle,
        SwapchainDesc, SwapchainHandle,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, SW_SHOWNOACTIVATE, ShowWindow, WS_EX_TOOLWINDOW, WS_POPUP,
    };
    use windows::core::PCWSTR;

    use crate::device::Dx12Device;
    use crate::device::tests::open_device;

    /// The window and swapchain size. Small, because nothing looks at the
    /// pixels and a back buffer is real memory on a runner with none to spare.
    const EXTENT: (u32, u32) = (320, 200);
    /// The size the reconfigure moves to. Different in both dimensions, so a
    /// resize that updated one and not the other is a red assertion rather than
    /// a coincidence.
    const RESIZED: (u32, u32) = (200, 320);
    /// Back buffers asked for, which with [`frame_latency`] makes the maximum
    /// frame latency two.
    const BUFFERS: u32 = 3;
    /// Long enough that a wait which genuinely blocked on a stalled compositor
    /// is a finding rather than a flake, short enough that a suite hitting it
    /// on every frame still finishes inside nextest's `slow-timeout`.
    const PRESENT_WAIT: Duration = Duration::from_secs(2);

    /// A real `HWND` for the length of a test, destroyed on drop.
    ///
    /// `"STATIC"` is one of the window classes user32 registers for every
    /// process, so this needs no `RegisterClassW` and no window procedure of
    /// its own — the message loop `crcbl-shell` owns is not what is under test
    /// here, only that DXGI is given a window that exists.
    ///
    /// `WS_POPUP` with no caption or border, so `GetClientRect` reports exactly
    /// the size asked for and `SurfaceCaps::current_extent` can be compared
    /// against a number this file wrote. `WS_EX_TOOLWINDOW` keeps it off the
    /// taskbar, and `SW_SHOWNOACTIVATE` makes it visible **without taking
    /// focus** — visible because a fully occluded window's presents complete
    /// differently, and without focus because the foreground window is one per
    /// session and `crcbl-shell`'s own e2e suite is entitled to it.
    struct TestWindow {
        hwnd: HWND,
    }

    impl TestWindow {
        fn open(size: (u32, u32)) -> Self {
            let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
            let title: Vec<u16> = "crcbl-dx12 swapchain\0".encode_utf16().collect();
            let width = i32::try_from(size.0).expect("a test window fits in an i32");
            let height = i32::try_from(size.1).expect("a test window fits in an i32");
            // SAFETY: both strings are NUL-terminated UTF-16 buffers that
            // outlive the call — `CreateWindowExW` copies what it needs — and
            // `"STATIC"` is a class user32 has registered in every process, so
            // the `None` module handle is the documented way to name it. Every
            // other argument is a scalar or an omitted optional.
            let hwnd = unsafe {
                CreateWindowExW(
                    WS_EX_TOOLWINDOW,
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
            .expect(
                "the windows-latest runner boots with a window station and a desktop; a failure \
                 here is a finding about the session, not a flaky test",
            );
            // SAFETY: `hwnd` is the window just created. The return value says
            // whether it was previously visible, which is not a failure.
            let _ = unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
            Self { hwnd }
        }

        /// The target a shell would hand the HAL for this window.
        ///
        /// `hinstance` is a dangling pointer on purpose: `Instance::create_surface`
        /// documents that this backend reads the window and nothing else —
        /// `CreateSwapChainForHwnd` takes no module handle — so there is
        /// nothing here that could dereference it. If that ever stops being
        /// true, this is the call site that has to change with it.
        fn target(&self) -> SurfaceTarget {
            SurfaceTarget::Win32 {
                hinstance: NonNull::dangling(),
                hwnd: NonNull::new(self.hwnd.0).expect("a created window is never null"),
            }
        }
    }

    impl Drop for TestWindow {
        fn drop(&mut self) {
            // SAFETY: `self.hwnd` is this object's own window, created on this
            // thread and not destroyed before now.
            if let Err(error) = unsafe { DestroyWindow(self.hwnd) } {
                log::debug!("crcbl-dx12: a test window would not close: {error}");
            }
        }
    }

    /// A three-buffer swapchain descriptor at `extent`, in whatever the surface
    /// says it prefers.
    fn configure(
        surface: SurfaceHandle,
        caps: &SurfaceCaps,
        extent: (u32, u32),
    ) -> SwapchainDesc<'static> {
        SwapchainDesc {
            label: Some("crcbl-dx12 e2e swapchain"),
            surface,
            format: caps
                .preferred_format()
                .expect("a flip-model surface always offers something"),
            extent,
            image_count: BUFFERS,
            present_mode: caps.choose_present_mode(&[PresentMode::Fifo]),
            composite_alpha: CompositeAlpha::Opaque,
        }
    }

    /// Clears the acquired frame and presents it, numbered `present_id`.
    ///
    /// The two barriers are the whole reason this records anything at all: a
    /// swapchain image is acquired in an undefined state and has to reach
    /// [`ResourceState::Present`] before `Present` sees it, which on D3D12 is a
    /// real `ResourceBarrier` pair the debug layer would otherwise report.
    fn draw_and_present(
        device: &Dx12Device,
        swapchain: SwapchainHandle,
        format: Format,
        present_id: u64,
    ) -> Result<(), SurfaceError> {
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");
        let frame = device.acquire_next_frame(swapchain)?;
        assert!(
            frame.acquire_semaphore.is_none() && frame.present_semaphore.is_none(),
            "D3D12 has an implicit acquire, so both semaphores are None: {frame:?}"
        );

        let range = ImageSubresourceRange::all(format);
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("crcbl-dx12 e2e frame"),
            queue,
        });
        encoder.pipeline_barrier(&Barriers {
            images: &[ImageBarrier::new(
                frame.image,
                range,
                ResourceState::Undefined,
                ResourceState::ColorAttachment,
            )],
            ..Barriers::default()
        });
        encoder.begin_render_pass(&RenderPassDesc {
            label: Some("crcbl-dx12 e2e clear"),
            color_attachments: &[ColorAttachment {
                view: frame.view,
                resolve: None,
                load: LoadOp::Clear,
                store: StoreOp::Store,
                clear: ClearValue::color([0.1, 0.2, 0.3, 1.0]),
            }],
            depth_stencil_attachment: None,
            render_area: Rect2d::from_size(frame.extent.0, frame.extent.1),
        });
        encoder.end_render_pass();
        encoder.pipeline_barrier(&Barriers {
            images: &[ImageBarrier::new(
                frame.image,
                range,
                ResourceState::ColorAttachment,
                ResourceState::Present,
            )],
            ..Barriers::default()
        });
        let commands = encoder
            .finish()
            .unwrap_or_else(|error| panic!("stage=finish id={present_id}: {error:?}"));
        device
            .submit(queue, &SubmitInfo::new(&[commands]))
            .unwrap_or_else(|error| panic!("stage=submit id={present_id}: {error:?}"));
        let presented = device.present(
            queue,
            &PresentInfo {
                swapchain,
                waits: &[],
                present_id: Some(present_id),
            },
        );
        device.destroy_command_buffer(commands);
        presented
    }

    /// **The whole slice, against a real window: a surface, its capabilities, a
    /// flip-model swapchain, four numbered frames, a pacing wait, a resize, and
    /// a teardown in the order obligation 2 requires.**
    ///
    /// **Nothing in this test has ever executed.** The development box is
    /// Linux; `windows-latest` is the first machine to run a line of it, which
    /// is also true of every other test in this crate. What is new here is that
    /// it needs a *window*, and the runner has one — `crcbl-shell`'s win32 e2e
    /// job creates real windows on the same image.
    ///
    /// What a green run proves, none of which any host-side test can:
    ///
    /// * `CreateSwapChainForHwnd` accepts this backend's `DXGI_SWAP_CHAIN_DESC1`
    ///   — the flip-discard effect, the linear buffer format under an sRGB
    ///   view, the waitable-object flag, and the tearing flag when the factory
    ///   reported one. A wrong field here is `E_INVALIDARG` and nothing else.
    /// * `GetFrameLatencyWaitableObject` hands back a handle
    ///   `WaitForSingleObjectEx` accepts, and a wait for a frame two back
    ///   returns rather than lapsing.
    /// * The render target views over the back buffers are real: a render pass
    ///   clears through one, with the acquire→attachment→present barrier pair
    ///   D3D12 requires.
    /// * `ResizeBuffers` succeeds after the views and images are released, and
    ///   the swapchain reports the new extent.
    ///
    /// The assertions that are *not* about the display are strict. The one that
    /// is — whether the pacing wait genuinely blocked — is **printed as a
    /// measurement** rather than demanded, because a window nobody is looking
    /// at is a state the compositor is entitled to treat differently, and the
    /// seam calls [`SurfaceError::Timeout`] expected traffic. Any *other* error
    /// from that wait is a failure. `docs/backlog.md` records what that leaves
    /// unmeasured.
    #[test]
    fn a_windowed_swapchain_presents_paces_and_resizes_on_a_real_hwnd() {
        let (instance, device) = open_device();
        let adapter = instance.adapters()[0].id;
        let window = TestWindow::open(EXTENT);

        // SAFETY: `window` owns a live `HWND` created on this thread, and it
        // outlives the surface and the swapchain — it is dropped at the end of
        // this function, after `destroy_surface` below. That is obligation 2's
        // teardown order, which this test is partly here to walk.
        let surface = unsafe { instance.create_surface(&window.target()) }
            .expect("a Win32 target is the one this backend presents to");

        let caps = instance
            .surface_caps(surface, adapter)
            .expect("a live surface on a real adapter");
        assert!(!caps.formats.is_empty(), "a surface offers something");
        assert!(
            caps.present_modes.contains(&PresentMode::Fifo),
            "the seam requires Fifo of every surface: {:?}",
            caps.present_modes
        );
        assert_eq!(
            caps.current_extent,
            Some(EXTENT),
            "current_extent must come from this window's client area, not from a constant"
        );
        assert!(caps.min_image_count >= 2, "flip-discard refuses one buffer");
        // The measurement this machine alone can make, printed the way
        // `crcbl_dx12::instance`'s adapter report is.
        println!("crcbl-dx12 surface caps: {caps:?}");

        let desc = configure(surface, &caps, EXTENT);
        let format = desc.format;
        let swapchain = device
            .create_swapchain(&desc)
            .expect("a flip-model swapchain on a real window");

        // An id nothing has presented, before anything has been presented at
        // all. This is the seam's immediate answer, and it is the assertion
        // that would cost a whole `PRESENT_WAIT` per frame if it regressed.
        let started = Instant::now();
        device
            .wait_until_presented(swapchain, 1, PRESENT_WAIT)
            .expect("nothing has been presented, so there is nothing to wait for");
        assert!(
            started.elapsed() < PRESENT_WAIT / 2,
            "an unpresented id blocked for {:?}",
            started.elapsed()
        );

        let mut indices: Vec<u32> = Vec::new();
        for id in 1..=4u64 {
            let frame = device.acquire_next_frame(swapchain).expect("a back buffer");
            assert_eq!(frame.extent, EXTENT, "obligation 3's render area");
            assert!(
                frame.index < BUFFERS,
                "index {} is past {BUFFERS} buffers",
                frame.index
            );
            assert!(!frame.suboptimal, "DXGI reports no such condition");
            indices.push(frame.index);

            draw_and_present(&device, swapchain, format, id)
                .unwrap_or_else(|error| panic!("stage=present id={id}: {error:?}"));

            // Two frames back, which is the cadence
            // `GpuContext::present_to_wait_for` uses and the one the frame
            // latency is set for.
            if let Some(waiting_for) = id.checked_sub(2).filter(|id| *id > 0) {
                let started = Instant::now();
                match device.wait_until_presented(swapchain, waiting_for, PRESENT_WAIT) {
                    Ok(()) => println!(
                        "crcbl-dx12: present wait for {waiting_for} returned after {:?}",
                        started.elapsed()
                    ),
                    Err(SurfaceError::Timeout) => println!(
                        "crcbl-dx12: present wait for {waiting_for} lapsed after {:?}; the \
                         compositor did not retire a frame for an unfocused window",
                        started.elapsed()
                    ),
                    Err(error) => panic!("stage=wait_until_presented id={waiting_for}: {error:?}"),
                }
            }
        }
        assert!(
            indices
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
                > 1,
            "GetCurrentBackBufferIndex never moved off one buffer: {indices:?}"
        );

        // **The resize, and the numbering restarting with it.** `ResizeBuffers`
        // is where a swapchain forgets every id it was given, which is what
        // `PresentInfo::present_id` documents and what would otherwise leave a
        // pacing loop blocking for a frame that no longer exists.
        let resized = configure(surface, &caps, RESIZED);
        device
            .reconfigure_swapchain(swapchain, &resized)
            .expect("ResizeBuffers after the views and images were released");
        let frame = device
            .acquire_next_frame(swapchain)
            .expect("a back buffer at the new size");
        assert_eq!(
            frame.extent, RESIZED,
            "the swapchain reports the size it was reconfigured at"
        );
        let started = Instant::now();
        device
            .wait_until_presented(swapchain, 4, PRESENT_WAIT)
            .expect("this swapchain object has presented nothing since the resize");
        assert!(
            started.elapsed() < PRESENT_WAIT / 2,
            "an id from before the reconfigure blocked for {:?}",
            started.elapsed()
        );
        draw_and_present(&device, swapchain, format, 5).expect("a frame at the new size");

        // Obligation 2's teardown order: the swapchain, then the surface, then
        // the device and the window. Reversing the first two is the case the
        // seam says must be a clean error rather than undefined behaviour, and
        // `SurfaceEntry` explains why on this backend it cannot be either.
        device.destroy_swapchain(swapchain);
        instance.destroy_surface(surface);
        device.wait_idle().expect("the queue drains");
    }

    /// **A swapchain outlives its surface handle, and a destroyed surface
    /// handle stops resolving.**
    ///
    /// Obligation 2 permits a caller to destroy the surface handle while a
    /// swapchain is still configured — it only forbids destroying the *window*
    /// — and requires the handle to be invalid immediately. On this backend the
    /// first half is structural, because the swapchain keeps its own
    /// `IDXGISwapChain3` and its own copy of the `HWND` and never reads the
    /// surface table again; this is what makes that claim a check rather than a
    /// sentence.
    ///
    /// Red if `SwapchainEntry` ever starts resolving its surface handle per
    /// frame (the acquire after the destroy then fails), and red if
    /// `destroy_surface` stops invalidating the handle (the `surface_caps`
    /// below then succeeds).
    #[test]
    fn a_swapchain_keeps_working_after_its_surface_handle_is_destroyed() {
        let (instance, device) = open_device();
        let adapter = instance.adapters()[0].id;
        let window = TestWindow::open(EXTENT);
        // SAFETY: as above — `window` outlives everything made from it.
        let surface = unsafe { instance.create_surface(&window.target()) }
            .expect("a Win32 target is the one this backend presents to");
        let caps = instance
            .surface_caps(surface, adapter)
            .expect("a live surface");
        let desc = configure(surface, &caps, EXTENT);
        let format = desc.format;
        let swapchain = device.create_swapchain(&desc).expect("a swapchain");

        instance.destroy_surface(surface);
        let error = instance
            .surface_caps(surface, adapter)
            .expect_err("the handle died the moment the caller said so");
        assert!(
            matches!(error, HalError::InvalidHandle { kind, .. } if kind == "surface"),
            "{error:?}"
        );

        // The swapchain is untouched by that, which is the point.
        draw_and_present(&device, swapchain, format, 1)
            .expect("a swapchain outlives its surface handle");
        device.destroy_swapchain(swapchain);
        device.wait_idle().expect("the queue drains");
    }

    /// **A descriptor D3D12 would reject is refused here, by name, before DXGI
    /// sees it.**
    ///
    /// Each of these reaches `CreateSwapChainForHwnd` as a bare `E_INVALIDARG`
    /// naming no field, which is the failure `crate::swapchain`'s `check`
    /// exists to turn into something a caller can act on. Asserted against a
    /// real window so the refusal is genuinely the descriptor's and not a
    /// handle failing first.
    ///
    /// Red when any of the three checks is dropped: the call then either
    /// succeeds or comes back as `HalError::Backend` with DXGI's message, and
    /// both fail the `InvalidDescriptor` assertion.
    #[test]
    fn a_descriptor_flip_discard_rejects_is_refused_before_dxgi_sees_it() {
        let (instance, device) = open_device();
        let adapter = instance.adapters()[0].id;
        let window = TestWindow::open(EXTENT);
        // SAFETY: as above.
        let surface = unsafe { instance.create_surface(&window.target()) }.expect("a surface");
        let caps = instance
            .surface_caps(surface, adapter)
            .expect("a live surface");
        let good = configure(surface, &caps, EXTENT);

        let bad: &[(&str, SwapchainDesc<'_>)] = &[
            (
                "a zero extent",
                SwapchainDesc {
                    extent: (0, 0),
                    ..good
                },
            ),
            (
                "a format flip-discard rejects",
                SwapchainDesc {
                    format: Format::Rgba32Float,
                    ..good
                },
            ),
            (
                "a composite-alpha mode an HWND swapchain has no spelling for",
                SwapchainDesc {
                    composite_alpha: CompositeAlpha::PreMultiplied,
                    ..good
                },
            ),
        ];
        assert!(!bad.is_empty(), "nothing to check");
        for (what, desc) in bad {
            let error = device
                .create_swapchain(desc)
                .expect_err("this descriptor is not configurable");
            let SurfaceError::Hal(HalError::InvalidDescriptor(message)) = &error else {
                panic!("{what}: expected a named descriptor refusal, got {error:?}");
            };
            println!("crcbl-dx12: {what} -> {message}");
        }

        instance.destroy_surface(surface);
    }
}
