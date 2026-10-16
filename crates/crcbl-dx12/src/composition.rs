//! DirectComposition: how a transparent window shows a swapchain.
//!
//! A window the shell made transparent
//! ([`SurfaceTarget::Win32`](crcbl_core::SurfaceTarget::Win32)'s
//! `transparent`) is created with `WS_EX_NOREDIRECTIONBITMAP`, so DWM keeps no
//! redirection surface for it and nothing presented to the `HWND` itself is
//! shown. What it shows is a DirectComposition visual tree bound to it, and
//! this module is that tree: one device, one target on the window, one visual
//! as the root, and a composition swapchain
//! (`IDXGIFactory2::CreateSwapChainForComposition`) as the visual's content.
//! That is Microsoft's documented shape for per-pixel alpha with a flip-model
//! swapchain, and DWM blends the visual by its premultiplied alpha.
//!
//! # Who owns what
//!
//! The [`Composition`] is made with the **surface** (`create_surface`), because
//! `CreateTargetForHwnd` may bind one target per window: a swapchain destroyed
//! and created again on the same surface reuses the target rather than racing
//! the old one's release. Each swapchain takes a [`Binding`], which holds the
//! composition alive too, so a swapchain still presents after its surface
//! handle is destroyed, as `crcbl-hal`'s obligation 2 permits.
//!
//! # Teardown order
//!
//! Releasing a COM reference does not take a visual off the screen; a
//! committed change does. So both drops below commit an emptier tree before
//! anything is released:
//!
//! 1. A [`Binding`] drops first (it is declared before the swapchain in
//!    `SwapchainEntry`): the visual's content is cleared and committed, which
//!    is DirectComposition's reference to the swapchain going away. Only then
//!    is the swapchain's last reference ours to release.
//! 2. The [`Composition`] drops when its last holder does: the target's root is
//!    cleared and committed, then the visual and the target are released, and
//!    the device last. The visual and target are the device's objects, and the
//!    device is what carries the commit to DWM, so it outlives both.
//!
//! # Hit testing is not by alpha
//!
//! A pixel of alpha zero is still the window's: the pointer hits it, unlike a
//! layered window's (`WS_EX_LAYERED` with per-pixel alpha), which passes a
//! fully transparent pixel through. Click-through is the application's, as on
//! any window: `WM_NCHITTEST` answering `HTTRANSPARENT`, which is what
//! `PopupInput::Passthrough` is in the Win32 shell.
//!
//! # A resize does not stretch
//!
//! A visual shows its swapchain at the swapchain's own size, from the window's
//! top-left corner: DirectComposition scales content only through a transform,
//! and none is set. So between a window resize and the swapchain's
//! `ResizeBuffers`, a grown window has a transparent band at its right and
//! bottom, and a shrunk one clips its last frame. An `HWND` swapchain stretches
//! instead (`DXGI_SCALING_STRETCH`, chosen in `crate::swapchain` for that
//! reason); on a transparent window a stretched stale frame would be the worse
//! of the two, because its alpha edges would move.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crcbl_hal::HalError;
use windows::Win32::Foundation::{DCOMPOSITION_ERROR_WINDOW_ALREADY_COMPOSED, HWND};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice2, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual,
};
use windows::Win32::Graphics::Dxgi::IDXGISwapChain1;
use windows::core::IUnknown;

/// A window's DirectComposition tree: the device, the target bound to the
/// window, and the root visual a swapchain is shown through.
///
/// Field order is drop order, and the order is the one the module docs give:
/// the visual and the target before the device that made them.
#[derive(Debug)]
pub(crate) struct Composition {
    visual: IDCompositionVisual,
    target: IDCompositionTarget,
    device: IDCompositionDevice,
    /// Whether a swapchain is the visual's content now. A second one would
    /// silently replace the first's content, so [`Composition::bind`] refuses
    /// it, as DXGI refuses a second flip-model swapchain on one `HWND`.
    bound: AtomicBool,
}

// SAFETY: `windows-rs` marks none of the DirectComposition interfaces `Send` or
// `Sync`, because its metadata does not declare them agile; it does not say
// they are thread-affine. Microsoft's "Basic concepts (DirectComposition)"
// does: "DirectComposition objects are not thread bound; you can use multiple
// threads to modify the same set of objects", and "Your application can call
// DirectComposition from multiple threads at the same time." They are not
// apartment COM objects (nothing here goes through `CoCreateInstance`), so no
// apartment rule applies either. What the documentation leaves to the caller
// is ordering between concurrent calls, and `bound` provides it: only the
// holder of the one live `Binding` changes the visual's content, the flag is
// given back only after that holder's last commit, and `Composition`'s own
// drop runs with the last reference.
unsafe impl Send for Composition {}
// SAFETY: as above.
unsafe impl Sync for Composition {}

impl Composition {
    /// Builds the tree for `hwnd`: a device with no rendering device of its
    /// own, a target on the window, and an empty root visual, committed.
    ///
    /// `topmost` is `false`, so the visual is composed above the window but
    /// below any child window it has. A shell window has none today; a child
    /// window added later would sit above the frame rather than under it.
    ///
    /// # Errors
    ///
    /// [`HalError::Backend`] naming the call that failed. One case is named
    /// further: a window that already has a target, which on this backend is a
    /// second live transparent surface on one window.
    pub(crate) fn for_window(hwnd: HWND) -> Result<Self, HalError> {
        // SAFETY: a null rendering device is documented as allowed for
        // `DCompositionCreateDevice2` when no DirectComposition surface will be
        // drawn into, and none is: the visual's content is a swapchain. The
        // call writes one interface pointer, which the binding wraps.
        let device: IDCompositionDevice =
            unsafe { DCompositionCreateDevice2(None::<&IUnknown>) }
                .map_err(|error| failed("DCompositionCreateDevice2", &error))?;
        // SAFETY: `device` is the live device just created, and `hwnd` is the
        // window `Instance::create_surface`'s caller promised is live and owned
        // by this process, which is what `CreateTargetForHwnd` requires.
        let target = unsafe { device.CreateTargetForHwnd(hwnd, false) }.map_err(|error| {
            if error.code() == DCOMPOSITION_ERROR_WINDOW_ALREADY_COMPOSED {
                HalError::Backend(format!(
                    "IDCompositionDevice::CreateTargetForHwnd failed: this window already has a \
                     DirectComposition target, which on crcbl-dx12 means another live \
                     transparent surface (or a swapchain made on one) is presenting to it: \
                     {error}"
                ))
            } else {
                failed("IDCompositionDevice::CreateTargetForHwnd", &error)
            }
        })?;
        // SAFETY: as above; the call takes no argument and returns a new
        // visual owned by `device`.
        let visual = unsafe { device.CreateVisual() }
            .map_err(|error| failed("IDCompositionDevice::CreateVisual", &error))?;
        // SAFETY: `visual` was created by `device`, which made `target`, so the
        // root is one of the target's own device's visuals, as required.
        unsafe { target.SetRoot(&visual) }
            .map_err(|error| failed("IDCompositionTarget::SetRoot", &error))?;
        // SAFETY: `device` is live; the call takes no argument.
        unsafe { device.Commit() }
            .map_err(|error| failed("IDCompositionDevice::Commit", &error))?;
        Ok(Self {
            visual,
            target,
            device,
            bound: AtomicBool::new(false),
        })
    }

    /// Makes `swapchain` the visual's content and commits it, which is what
    /// puts its frames on the window.
    ///
    /// Presenting needs no further commit: the visual shows whatever buffer
    /// the swapchain last presented.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidDescriptor`] while another swapchain is bound, and
    /// [`HalError::Backend`] if DirectComposition refused the content or the
    /// commit.
    pub(crate) fn bind(self: &Arc<Self>, swapchain: &IDXGISwapChain1) -> Result<Binding, HalError> {
        if self
            .bound
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(HalError::InvalidDescriptor(
                "a swapchain is already presenting to this transparent window; destroy it \
                 before creating another"
                    .to_string(),
            ));
        }
        // A `Binding` from here on, so an error below still clears what was
        // set and gives the flag back.
        let binding = Binding {
            composition: Arc::clone(self),
        };
        // SAFETY: `visual` is live and `swapchain` is a composition swapchain
        // (`CreateSwapChainForComposition`), the content type `SetContent`
        // documents; the visual takes its own reference to it.
        unsafe { self.visual.SetContent(swapchain) }
            .map_err(|error| failed("IDCompositionVisual::SetContent", &error))?;
        // SAFETY: `device` is live; the call takes no argument.
        unsafe { self.device.Commit() }
            .map_err(|error| failed("IDCompositionDevice::Commit", &error))?;
        Ok(binding)
    }
}

impl Drop for Composition {
    fn drop(&mut self) {
        // SAFETY: `target` is live until the fields drop after this body.
        // `None` clears the root, which is documented.
        let cleared = unsafe { self.target.SetRoot(None::<&IDCompositionVisual>) }.and_then(|()| {
            // SAFETY: `device` is live, as above; the call takes no argument.
            unsafe { self.device.Commit() }
        });
        if let Err(error) = cleared {
            // Logged, not propagated: a drop has nowhere to send it, and the
            // device's release below takes the tree down with it anyway.
            crcbl_core::log::debug!(
                "crcbl-dx12: clearing a DirectComposition root before release failed: {error}"
            );
        }
    }
}

/// One swapchain's claim on a [`Composition`]: it is the visual's content
/// while this lives.
///
/// Dropping it clears the content and commits, which is what lets the
/// swapchain's last reference go, and keeps the composition alive until then.
#[derive(Debug)]
pub(crate) struct Binding {
    composition: Arc<Composition>,
}

impl Drop for Binding {
    fn drop(&mut self) {
        let composition = &self.composition;
        // SAFETY: the visual is live while `composition` is. `None` clears
        // the content, which is documented.
        let cleared = unsafe { composition.visual.SetContent(None::<&IUnknown>) }.and_then(|()| {
            // SAFETY: the device is live, as above; the call takes no argument.
            unsafe { composition.device.Commit() }
        });
        if let Err(error) = cleared {
            crcbl_core::log::debug!(
                "crcbl-dx12: clearing a DirectComposition visual's swapchain failed: {error}"
            );
        }
        composition.bound.store(false, Ordering::Release);
    }
}

/// A DirectComposition failure, with the call named.
fn failed(what: &str, error: &windows::core::Error) -> HalError {
    HalError::Backend(format!("{what} failed: {error}"))
}
