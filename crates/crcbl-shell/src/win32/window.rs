//! The window class, window creation, and the borderless round trip.
//!
//! # The class is registered once per process and never unregistered
//!
//! A window class is a **process-wide** registration keyed by name, not a
//! per-shell resource. Registering it in [`Win32Shell::open`](super::Win32Shell)
//! and unregistering it on drop would look tidier and would be wrong twice: a
//! second shell in the same process would fail to register a name that already
//! exists, and `UnregisterClassW` refuses while any window of the class is
//! alive — which, during a `Drop` that has just asked for its windows to be
//! destroyed, is a race rather than a certainty.
//!
//! So the registration is behind a [`OnceLock`] with the process's lifetime.
//! `ERROR_CLASS_ALREADY_EXISTS` is treated as success for the case the lock
//! cannot see: the same code loaded twice, in two modules. Windows are created
//! **by name** rather than by the returned atom, so that path needs nothing
//! else to work.
//!
//! # Borderless is a style swap, and the way back has to be exact
//!
//! `docs/plan/15-windowing.md` keeps two display modes and drops exclusive
//! fullscreen, so borderless here is the recipe every Windows game uses:
//! remember the window's style, extended style and *placement*, switch to
//! `WS_POPUP`, and `SetWindowPos` it over the target monitor's rectangle. The
//! desktop's video mode is never touched, which is the whole reason the mode
//! exists.
//!
//! The half that is easy to get wrong is the way back. Restoring the style and
//! the window rectangle is not enough: a window that was **maximized** when it
//! went borderless has to come back maximized, and its restored-down rectangle
//! has to survive too, or the user's next un-maximize puts the window somewhere
//! it has never been. `WINDOWPLACEMENT` carries all three — show state,
//! maximized position and normal rectangle — which is why it is what gets saved
//! rather than a `GetWindowRect`.

use core::ptr;
use std::sync::OnceLock;

use crate::{
    DisplayMode, MonitorId, MonitorInfo, PhysicalSize, Shell, ShellBackend, ShellError,
    WindowConfiguration,
};

use super::ffi::{self, Handle, Rect, WindowPlacement, WndClassExW, style, value};
use super::geometry::{self, Frame, Styles};
use super::proc::{self, Limits};
use super::shell::{Saved, Win32Shell, WinWindow};

/// The window class every window of this backend belongs to.
///
/// Not a per-process-unique name on purpose: two copies of this engine in one
/// process should share one class, and the [`OnceLock`] plus the
/// already-exists fallback makes that safe.
const CLASS_NAME: &str = "crcbl_window";

/// The registration, once per process. `Err` carries the `GetLastError` code.
static CLASS: OnceLock<Result<(), u32>> = OnceLock::new();

/// The class name as the NUL-terminated UTF-16 every `*W` entry point wants.
fn class_name() -> Vec<u16> {
    ffi::wide(CLASS_NAME).expect("the class name is a literal with no interior NUL")
}

/// Registers the window class, or reports why the process cannot have one.
///
/// This is the call that fails on a machine — or a service account — with no
/// usable window station, which makes it the honest place for
/// [`Win32Shell::open`](super::Win32Shell) to give up. Everything after it
/// assumes a desktop.
///
/// # Errors
///
/// [`ShellError::Connect`] with the `GetLastError` code, which is the only
/// thing the system says about it.
pub(super) fn register_class(instance: Handle) -> Result<(), ShellError> {
    let outcome = *CLASS.get_or_init(|| {
        let name = class_name();
        // SAFETY: a null instance with an integer resource id asks for a system
        // cursor, which is the documented use of `IDC_ARROW`. The handle is
        // owned by the system and must not be freed.
        let cursor = unsafe { ffi::LoadCursorW(ptr::null_mut(), value::IDC_ARROW) };
        // SAFETY: `GetStockObject` takes an index and returns a system-owned
        // brush that must not be deleted. Black rather than null, so a window
        // that has not rendered yet is black instead of showing whatever was
        // behind it.
        let brush = unsafe { ffi::GetStockObject(value::BLACK_BRUSH) };
        let class = WndClassExW {
            cb_size: size_of::<WndClassExW>() as u32,
            style: value::CS_REDRAW,
            lpfn_wnd_proc: proc::window_proc,
            cb_cls_extra: 0,
            cb_wnd_extra: 0,
            h_instance: instance,
            h_icon: ptr::null_mut(),
            h_cursor: cursor,
            hbr_background: brush,
            lpsz_menu_name: ptr::null(),
            lpsz_class_name: name.as_ptr(),
            h_icon_sm: ptr::null_mut(),
        };
        // SAFETY: `class` is a fully initialised `WNDCLASSEXW` whose `cb_size`
        // is its own size, `name` outlives the call — the system copies the
        // string into its own atom table before returning — and
        // `proc::window_proc` is a real `extern "system"` function with the
        // `WNDPROC` signature.
        let atom = unsafe { ffi::RegisterClassExW(&raw const class) };
        if atom != 0 {
            return Ok(());
        }
        // SAFETY: reads this thread's last error code, which the failed call
        // above set.
        let error = unsafe { ffi::GetLastError() };
        if error == value::ERROR_CLASS_ALREADY_EXISTS {
            // Another copy of this code, in another module of the same
            // process. The class it registered is ours, by name and by window
            // procedure, so creating windows against it is correct.
            log::debug!("the {CLASS_NAME} class was already registered in this process");
            return Ok(());
        }
        Err(error)
    });
    outcome.map_err(|error| ShellError::Connect {
        backend: ShellBackend::Win32,
        detail: format!(
            "RegisterClassExW failed with Win32 error {error} — a process with no usable \
             window station cannot create windows"
        ),
    })
}

/// What [`create_native_window`](Win32Shell::create_native_window) hands back.
#[derive(Clone, Copy, Debug)]
pub(super) struct Created {
    /// The window.
    pub hwnd: Handle,
    /// Its DPI, read back after creation rather than assumed.
    pub dpi: u32,
    /// Its client area, which the system already knows.
    pub size: PhysicalSize,
}

impl Win32Shell {
    /// The non-client padding a window of these styles has at this DPI.
    ///
    /// `AdjustWindowRectExForDpi` against an empty rectangle: what it expands
    /// the rectangle *by* is exactly the frame. Doing it per DPI rather than
    /// once is not ceremony — the border width and caption height are both
    /// scaled, so a 4 K monitor at 200% has twice the frame of a 1080p one, and
    /// a minimum size computed with the wrong one is wrong by tens of pixels.
    pub(super) fn frame_for(styles: Styles, dpi: u32) -> Frame {
        let mut rect = Rect::default();
        // SAFETY: `rect` is a live, initialised `RECT` the call expands in
        // place; `menu = FALSE` because no window of this backend has a menu
        // bar, and a wrong answer there is a caption height of difference.
        let ok = unsafe {
            ffi::AdjustWindowRectExForDpi(
                &raw mut rect,
                styles.style,
                0,
                styles.ex_style,
                dpi.max(1),
            )
        };
        if ok == 0 {
            // Only reachable if the call rejected the style word. A zero frame
            // makes the window slightly smaller than asked rather than
            // refusing to create it.
            log::debug!("AdjustWindowRectExForDpi refused style {:#x}", styles.style);
            return Frame::default();
        }
        Frame {
            width: rect.width(),
            height: rect.height(),
        }
    }

    /// A window's current DPI, or the 100% default if the system will not say.
    pub(super) fn dpi_of(hwnd: Handle) -> u32 {
        // SAFETY: `hwnd` is a live window of this shell.
        let dpi = unsafe { ffi::GetDpiForWindow(hwnd) };
        if dpi == 0 { value::DEFAULT_DPI } else { dpi }
    }

    /// A window's client area as the system has it right now.
    ///
    /// The synchronous read X11 also has and Wayland does not: the size exists
    /// before anybody asks for it. It is still delivered as an event rather
    /// than returned from `create_window` — see
    /// [`create_window`](crate::Shell::create_window) on this backend.
    pub(super) fn client_size_of(hwnd: Handle) -> PhysicalSize {
        let mut rect = Rect::default();
        // SAFETY: `rect` is a live, initialised `RECT` the call writes into,
        // and `hwnd` is a live window.
        let ok = unsafe { ffi::GetClientRect(hwnd, &raw mut rect) };
        if ok == 0 {
            return PhysicalSize::default();
        }
        geometry::client_size(rect)
    }

    /// Creates the `HWND` for a descriptor, positioned on `target`.
    ///
    /// # Why the DPI is read back
    ///
    /// A window's DPI is a property of the monitor it is *on*, and it does not
    /// exist until the window does. The size is therefore computed from the
    /// target monitor's DPI — the best guess available — and then checked: if
    /// the system put the window somewhere else, the client area is recomputed
    /// at the real DPI so that the *logical* size the caller asked for is what
    /// they get. Skipping this is how a window asked for at 1280×720 opens at
    /// 1280×720 physical pixels on a 200% monitor, which is half the size the
    /// caller meant.
    ///
    /// # Errors
    ///
    /// [`ShellError::WindowCreation`] carrying the `GetLastError` code.
    pub(super) fn create_native_window(
        &self,
        desc: &crate::WindowDesc<'_>,
        target: Option<&MonitorInfo>,
    ) -> Result<Created, ShellError> {
        let title = ffi::wide(desc.title).ok_or_else(|| {
            ShellError::InvalidDescriptor("title contains a NUL byte".to_string())
        })?;
        let styles = geometry::styles(desc.mode, desc.resizable);
        let dpi = target.map_or(value::DEFAULT_DPI, |monitor| {
            // The monitor's scale is its DPI divided by 96, so this recovers
            // the DPI the shell already asked the system for.
            (monitor.scale_factor * f64::from(value::DEFAULT_DPI)).round() as u32
        });
        let frame = Self::frame_for(styles, dpi);

        // A borderless window is the monitor's size, not the requested one:
        // `WindowDesc::size` is the *windowed* size and is remembered for the
        // trip back.
        let client = match (desc.mode, target) {
            (DisplayMode::Borderless { .. }, Some(monitor)) => monitor.bounds.size(),
            _ => desc.size.to_physical(geometry::scale_from_dpi(dpi)),
        };
        let outer = frame.window_size((
            i32::try_from(client.width).unwrap_or(i32::MAX),
            i32::try_from(client.height).unwrap_or(i32::MAX),
        ));
        let (x, y) = match (desc.mode, target) {
            (DisplayMode::Borderless { .. }, Some(monitor)) => (monitor.bounds.x, monitor.bounds.y),
            (_, Some(monitor)) => geometry::centred(monitor.work_area, outer),
            // No monitor to place against — a session with no display attached.
            // `CW_USEDEFAULT` lets the system decide rather than inventing a
            // desktop layout.
            (_, None) => (value::CW_USE_DEFAULT, value::CW_USE_DEFAULT),
        };

        let mut window_style = styles.style;
        if desc.visible {
            window_style |= style::VISIBLE;
        }
        let name = class_name();
        // SAFETY: `name` and `title` are NUL-terminated UTF-16 that outlive the
        // call; the class was registered by `register_class` on this instance;
        // a null parent makes this a top-level window and a null menu is
        // correct for one; and `param` is a pointer to the `Shared` this shell
        // owns, which the window procedure reads in `WM_NCCREATE` and which
        // outlives every window (see `Win32Shell::drop`). The window procedure
        // runs re-entrantly inside this call — that is what delivers
        // `WM_NCCREATE`, `WM_CREATE` and the first `WM_SIZE` — so nothing here
        // may hold a `RefCell` borrow, and nothing does.
        let hwnd = unsafe {
            ffi::CreateWindowExW(
                styles.ex_style,
                name.as_ptr(),
                title.as_ptr(),
                window_style,
                x,
                y,
                outer.0,
                outer.1,
                ptr::null_mut(),
                ptr::null_mut(),
                self.instance(),
                self.shared_ptr(),
            )
        };
        if hwnd.is_null() {
            // SAFETY: reads this thread's last error code.
            let error = unsafe { ffi::GetLastError() };
            return Err(ShellError::WindowCreation(format!(
                "CreateWindowExW failed with Win32 error {error}"
            )));
        }

        if desc.accept_drops {
            // Off by default and opt-in per window, and here the opt-in is
            // literal: this sets `WS_EX_ACCEPTFILES`, and the system sends
            // `WM_DROPFILES` to no window without it. See `dnd`.
            super::dnd::accept_files(hwnd, true);
        }

        let real_dpi = Self::dpi_of(hwnd);
        let real_frame = Self::frame_for(styles, real_dpi);
        if real_dpi != dpi && !desc.mode.is_borderless() {
            // The window landed on a monitor at another scale. Recompute at the
            // DPI it actually has, so the logical size is honoured.
            let client = desc.size.to_physical(geometry::scale_from_dpi(real_dpi));
            let outer = real_frame.window_size((
                i32::try_from(client.width).unwrap_or(i32::MAX),
                i32::try_from(client.height).unwrap_or(i32::MAX),
            ));
            // SAFETY: resizing this shell's own window; the null insert-after
            // is unused because of `SWP_NO_Z_ORDER`.
            unsafe {
                ffi::SetWindowPos(
                    hwnd,
                    ptr::null_mut(),
                    0,
                    0,
                    outer.0,
                    outer.1,
                    value::SWP_NO_MOVE | value::SWP_NO_Z_ORDER | value::SWP_NO_ACTIVATE,
                );
            }
        }

        Ok(Created {
            hwnd,
            dpi: real_dpi,
            size: Self::client_size_of(hwnd),
        })
    }

    /// Publishes the numbers the window procedure answers `WM_GETMINMAXINFO`
    /// and `WM_SIZING` from.
    ///
    /// Called whenever any input to them moves: the constraints, the style
    /// (a mode change), or the DPI. Forgetting one of the three is a window
    /// that enforces last week's minimum size.
    pub(super) fn refresh_limits(&self, window: &WinWindow) {
        let styles = geometry::styles(window.effective_mode, window.resizable);
        let frame = Self::frame_for(styles, window.dpi);
        self.shared().set_limits(Limits {
            hwnd: window.key,
            track: geometry::track_sizes(window.requested_constraints, window.scale_factor, frame),
            aspect: window.requested_constraints.aspect,
            frame,
        });
    }

    /// Switches a window between windowed and borderless.
    ///
    /// The system is the only party involved — there is no window manager to
    /// ask — so this either succeeds or the monitor it named is gone. See the
    /// [module docs](self) for why the way back goes through
    /// `WINDOWPLACEMENT`.
    ///
    /// # Errors
    ///
    /// [`ShellError::NoSuchMonitor`] if the named monitor was unplugged since
    /// [`monitors`](crate::Shell::monitors) was called.
    pub(super) fn apply_mode(
        &mut self,
        window: crate::WindowId,
        mode: DisplayMode,
    ) -> Result<(), ShellError> {
        let state = self.window(window)?;
        let hwnd = state.raw();
        let resizable = state.resizable;
        let already_saved = state.saved.is_some();

        match mode {
            DisplayMode::Borderless { monitor } => {
                let bounds = self.borderless_bounds(hwnd, monitor)?;
                if !already_saved {
                    let saved = Saved {
                        // SAFETY: reading this shell's own window's styles.
                        style: unsafe { ffi::GetWindowLongPtrW(hwnd, value::GWL_STYLE) } as u32,
                        ex_style: unsafe { ffi::GetWindowLongPtrW(hwnd, value::GWL_EX_STYLE) }
                            as u32,
                        placement: Self::placement_of(hwnd),
                    };
                    self.window_mut(window)?.saved = Some(saved);
                }
                let styles = geometry::styles(mode, resizable);
                // `WS_VISIBLE` is state, not style: dropping it here would hide
                // a window that was on screen a moment ago.
                let visible = self
                    .window(window)?
                    .saved
                    .map_or(0, |saved| saved.style & style::VISIBLE);
                Self::set_styles(hwnd, styles.style | visible, styles.ex_style);
                // SAFETY: moving and resizing this shell's own window.
                // `SWP_FRAME_CHANGED` is required after a style change or the
                // system keeps drawing the old caption.
                unsafe {
                    ffi::SetWindowPos(
                        hwnd,
                        ptr::null_mut(),
                        bounds.x,
                        bounds.y,
                        i32::try_from(bounds.width).unwrap_or(i32::MAX),
                        i32::try_from(bounds.height).unwrap_or(i32::MAX),
                        value::SWP_NO_Z_ORDER
                            | value::SWP_NO_OWNER_Z_ORDER
                            | value::SWP_FRAME_CHANGED
                            | value::SWP_NO_ACTIVATE,
                    );
                }
            }
            DisplayMode::Windowed => {
                if let Some(saved) = self.window_mut(window)?.saved.take() {
                    Self::set_styles(hwnd, saved.style, saved.ex_style);
                    // Frame first, placement second: `SetWindowPlacement`
                    // positions the window for the style it has, so restoring
                    // the rectangle before the style would put a captioned
                    // window where a frameless one fitted.
                    //
                    // SAFETY: this shell's own window; nothing is moved or
                    // resized here, only the frame recalculated.
                    unsafe {
                        ffi::SetWindowPos(
                            hwnd,
                            ptr::null_mut(),
                            0,
                            0,
                            0,
                            0,
                            value::SWP_NO_MOVE
                                | value::SWP_NO_SIZE
                                | value::SWP_NO_Z_ORDER
                                | value::SWP_NO_OWNER_Z_ORDER
                                | value::SWP_FRAME_CHANGED
                                | value::SWP_NO_ACTIVATE,
                        );
                    }
                    // SAFETY: `placement` is a live, initialised
                    // `WINDOWPLACEMENT` whose `length` is its own size.
                    unsafe { ffi::SetWindowPlacement(hwnd, &raw const saved.placement) };
                } else if self.window(window)?.effective_mode.is_borderless() {
                    // **A window created borderless has nothing saved**, and
                    // there is nothing dishonest to do about it: it has never
                    // had a windowed style or a windowed rectangle. So one is
                    // built — the style for `Windowed`, and
                    // [`WindowDesc::size`](crate::WindowDesc::size) at the
                    // window's current scale, centred on the monitor it was
                    // covering.
                    //
                    // Without this branch the style would stay `WS_POPUP` while
                    // `effective_mode` said `Windowed`, which is the seam
                    // reporting a mode the window is not in — worse than any
                    // placement choice made here.
                    let state = self.window(window)?;
                    let styles = geometry::styles(DisplayMode::Windowed, resizable);
                    let frame = Self::frame_for(styles, state.dpi);
                    let client = state.requested_size.to_physical(state.scale_factor);
                    let outer = frame.window_size((
                        i32::try_from(client.width).unwrap_or(i32::MAX),
                        i32::try_from(client.height).unwrap_or(i32::MAX),
                    ));
                    let (x, y) = self
                        .monitor_of(hwnd)
                        .and_then(|id| self.monitors().iter().find(|info| info.id == id))
                        .map_or((0, 0), |info| geometry::centred(info.work_area, outer));
                    // SAFETY: reading this shell's own window's style, only to
                    // carry `WS_VISIBLE` across the change.
                    let visible = unsafe { ffi::GetWindowLongPtrW(hwnd, value::GWL_STYLE) } as u32
                        & style::VISIBLE;
                    Self::set_styles(hwnd, styles.style | visible, styles.ex_style);
                    // SAFETY: moving and resizing this shell's own window;
                    // `SWP_FRAME_CHANGED` is what makes the new caption appear.
                    unsafe {
                        ffi::SetWindowPos(
                            hwnd,
                            ptr::null_mut(),
                            x,
                            y,
                            outer.0,
                            outer.1,
                            value::SWP_NO_Z_ORDER
                                | value::SWP_NO_OWNER_Z_ORDER
                                | value::SWP_FRAME_CHANGED
                                | value::SWP_NO_ACTIVATE,
                        );
                    }
                }
            }
        }

        // Win32 has nobody to refuse: the effective mode is what was just
        // applied. The monitor is read back rather than assumed, because
        // `Borderless { monitor: None }` means "wherever the window already
        // is" and the answer names it.
        let effective_monitor = self.monitor_of(hwnd);
        let dpi = Self::dpi_of(hwnd);
        let state = self.window_mut(window)?;
        state.effective_mode = match mode {
            DisplayMode::Windowed => DisplayMode::Windowed,
            DisplayMode::Borderless { .. } => DisplayMode::Borderless {
                monitor: effective_monitor,
            },
        };
        state.dpi = dpi;
        state.scale_factor = geometry::scale_from_dpi(dpi);
        // The frame changed with the style, so the constraints the window
        // procedure enforces have to be recomputed against the new one.
        let state = self.window(window)?;
        self.refresh_limits(state);
        Ok(())
    }

    /// The rectangle a borderless window should cover.
    ///
    /// A named monitor is looked up in the enumeration — so an unplugged one is
    /// a clean [`ShellError::NoSuchMonitor`] rather than a window moved to
    /// nowhere — and `None` means the monitor the window is already on, which
    /// is what [`DisplayMode::Borderless`]'s field documents.
    fn borderless_bounds(
        &self,
        hwnd: Handle,
        monitor: Option<MonitorId>,
    ) -> Result<crate::PhysicalRect, ShellError> {
        if let Some(monitor) = monitor {
            return self
                .monitors()
                .iter()
                .find(|info| info.id == monitor)
                .map(|info| info.bounds)
                .ok_or(ShellError::NoSuchMonitor(monitor.0));
        }
        self.monitor_of(hwnd)
            .and_then(|id| self.monitors().iter().find(|info| info.id == id))
            // A hotplug between `MonitorFromWindow` and the enumeration this
            // shell holds: the primary is a better answer than a failure.
            .or_else(|| self.monitors().iter().find(|info| info.is_primary))
            .or_else(|| self.monitors().first())
            .map(|info| info.bounds)
            .ok_or_else(|| {
                ShellError::Backend(
                    "there is no monitor to place a borderless window on".to_string(),
                )
            })
    }

    /// Which of the enumerated monitors a window is on.
    ///
    /// `MONITOR_DEFAULTTONEAREST`, so a window dragged half off the desktop
    /// still answers. `None` only when the enumeration is empty or the system
    /// named a monitor this shell has not seen — a hotplug between the two.
    pub(super) fn monitor_of(&self, hwnd: Handle) -> Option<MonitorId> {
        // SAFETY: `hwnd` is a live window; the returned `HMONITOR` is a
        // system-owned pseudo-handle that needs no release.
        let handle = unsafe { ffi::MonitorFromWindow(hwnd, value::MONITOR_DEFAULT_TO_NEAREST) };
        if handle.is_null() {
            return None;
        }
        let device = super::monitors::device_name(handle)?;
        self.monitor_id_of(&device)
    }

    /// A window's `WINDOWPLACEMENT`, or a zeroed one if the call fails.
    fn placement_of(hwnd: Handle) -> WindowPlacement {
        let mut placement = WindowPlacement {
            length: size_of::<WindowPlacement>() as u32,
            ..WindowPlacement::default()
        };
        // SAFETY: `placement` is a live, initialised `WINDOWPLACEMENT` whose
        // `length` field the system validates against its own idea of the
        // structure's size.
        unsafe { ffi::GetWindowPlacement(hwnd, &raw mut placement) };
        placement
    }

    /// Writes both style words, carrying the extended bits that are *state*.
    ///
    /// `WS_EX_ACCEPTFILES` is what `DragAcceptFiles` sets, so it is a fact about
    /// the window rather than a consequence of its display mode — exactly like
    /// `WS_VISIBLE` in the word next door, which the callers carry across by
    /// hand. Recomputing the extended style from
    /// [`geometry::styles`](super::geometry::styles) and writing it back
    /// without this bit stops a window receiving `WM_DROPFILES` the first time
    /// it goes borderless, with nothing anywhere reporting that drops stopped
    /// working.
    fn set_styles(hwnd: Handle, window_style: u32, ex_style: u32) {
        // SAFETY: reading this shell's own window's extended style, only to
        // carry the drop registration across the write below.
        let accepts_drops = unsafe { ffi::GetWindowLongPtrW(hwnd, value::GWL_EX_STYLE) } as u32
            & style::EX_ACCEPT_FILES;
        // SAFETY: setting this shell's own window's styles. The values are
        // widened through `u32` on purpose — the style words are 32 bits even
        // on 64-bit Windows, and sign-extending them through `isize` would set
        // every high bit for a style with `WS_POPUP` in it.
        unsafe {
            ffi::SetWindowLongPtrW(hwnd, value::GWL_STYLE, window_style as isize);
            ffi::SetWindowLongPtrW(
                hwnd,
                value::GWL_EX_STYLE,
                (ex_style | accepts_drops) as isize,
            );
        }
    }

    /// The configuration a window has right now, for publishing on the next
    /// pump.
    pub(super) fn configuration_of(window: &WinWindow) -> WindowConfiguration {
        WindowConfiguration {
            size: Self::client_size_of(window.raw()),
            scale_factor: window.scale_factor,
            mode: window.effective_mode,
        }
    }
}
