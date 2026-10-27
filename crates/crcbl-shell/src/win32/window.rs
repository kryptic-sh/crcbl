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
//! The windowing rules (`docs/notes/backends.md`) keep two display modes and drop exclusive
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

use super::ffi::{self, Handle, Rect, WindowPlacement, WndClassExW, dwm, style, value};
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
pub(super) fn class_name() -> Vec<u16> {
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
            h_icon: module_icon(instance, ffi::value::SM_CX_ICON),
            h_cursor: cursor,
            hbr_background: brush,
            lpsz_menu_name: ptr::null(),
            lpsz_class_name: name.as_ptr(),
            h_icon_sm: module_icon(instance, ffi::value::SM_CX_SM_ICON),
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
            crcbl_core::log::debug!(
                "the {CLASS_NAME} class was already registered in this process"
            );
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

/// `module`'s icon at the side the system metric `metric` gives, or null
/// when the module carries none, which leaves the system's default icon.
///
/// **The icon Explorer shows for the executable's file**: its first icon group
/// in resource-directory order (see [`first_icon_group`]), so a program that
/// embeds an icon gets it on its windows' captions and taskbar buttons without
/// telling the shell anything. Loaded at the class's registration, which
/// happens once and is never undone, so the icon lives as long as the process
/// and is never destroyed.
fn module_icon(module: Handle, metric: i32) -> Handle {
    let Some(name) = first_icon_group(module) else {
        return ptr::null_mut();
    };
    // SAFETY: `GetSystemMetrics` takes an index and returns a number.
    let side = unsafe { ffi::GetSystemMetrics(metric) };
    // SAFETY: `module` is a loaded module, and `name` is the integer id or the
    // NUL-terminated string `first_icon_group` read out of its resources, kept
    // alive for the call. Flags zero: a new icon of exactly that size, picked
    // from the group's images.
    let icon =
        unsafe { ffi::LoadImageW(module, name.as_ptr(), ffi::value::IMAGE_ICON, side, side, 0) };
    if icon.is_null() {
        // SAFETY: reads this thread's last error, which the call above set.
        let error = unsafe { ffi::GetLastError() };
        crcbl_core::log::debug!(
            "the executable's icon would not load at {side}px (Win32 error {error}); its \
             windows keep the default icon"
        );
    }
    icon
}

/// A resource's name: an integer id or a string, as Win32 names one.
#[derive(Clone, Debug, PartialEq, Eq)]
enum ResourceName {
    Id(u16),
    /// NUL-terminated UTF-16.
    Named(Vec<u16>),
}

impl ResourceName {
    /// The name as Win32 takes it: an id as `MAKEINTRESOURCEW` spells it, a
    /// string by address, valid while `self` is.
    fn as_ptr(&self) -> *const u16 {
        match self {
            Self::Id(id) => usize::from(*id) as *const u16,
            Self::Named(name) => name.as_ptr(),
        }
    }
}

/// The name of `module`'s first icon group, the one Explorer shows for its
/// file, or `None` when it has none.
///
/// First in resource-directory order, which is the order
/// `EnumResourceNamesW` walks: named groups alphabetically, then numbered
/// ones by id. Not resource id 1 by assumption: a program may number its icon
/// otherwise, or name it.
fn first_icon_group(module: Handle) -> Option<ResourceName> {
    /// Keeps the first name and stops the walk.
    ///
    /// # Safety
    ///
    /// `param` is the address of the `Option<ResourceName>` below, and `name`
    /// is an integer id or a NUL-terminated string valid for this call, as
    /// `EnumResourceNamesW` documents.
    unsafe extern "system" fn take_first(
        _module: Handle,
        _kind: *const u16,
        name: *mut u16,
        param: ffi::Lparam,
    ) -> ffi::Bool32 {
        let raw = name as usize;
        let name = if raw >> 16 == 0 {
            ResourceName::Id(raw as u16)
        } else {
            let mut len = 0;
            // SAFETY: a string name is NUL-terminated, so every read up to and
            // including the terminator is inside it.
            while unsafe { *name.add(len) } != 0 {
                len += 1;
            }
            // SAFETY: the `len + 1` units just walked, terminator included.
            ResourceName::Named(unsafe { core::slice::from_raw_parts(name, len + 1) }.to_vec())
        };
        // SAFETY: `param` is the live `Option` `first_icon_group` passed, and
        // nothing else touches it while the walk runs.
        unsafe { *(param as *mut Option<ResourceName>) = Some(name) };
        0
    }

    let mut found: Option<ResourceName> = None;
    // SAFETY: `module` is a loaded module, the type is an integer resource
    // id, `take_first` has the `ENUMRESNAMEPROCW` signature, and `found`
    // outlives the call, which runs the callback only before it returns. The
    // result is not read: the walk stopped by the callback reports failure
    // too, and a module with no icon group reports it the same way.
    unsafe {
        ffi::EnumResourceNamesW(
            module,
            ffi::value::RT_GROUP_ICON as *const u16,
            take_first,
            core::ptr::from_mut(&mut found) as ffi::Lparam,
        );
    }
    found
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
            crcbl_core::log::debug!("AdjustWindowRectExForDpi refused style {:#x}", styles.style);
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
        let styles = geometry::with_transparency(
            geometry::styles(desc.mode, desc.resizable),
            desc.transparent,
        );
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
        // The input method stays out until a text field asks for it, as
        // `Shell::set_text_input` says of every window.
        if let Err(error) = super::input::enable_ime(hwnd, false) {
            crcbl_core::log::warn!("{error}; the input method stays on for this window");
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
        let styles = match (&window.panel, window.popup.map(|popup| popup.input)) {
            (Some(panel), _) => geometry::panel_styles(panel.layer, panel.keyboard),
            // The topmost bit does not change the frame, which is all this is
            // read for.
            (None, Some(input)) => geometry::popup_styles(input, false),
            (None, None) => geometry::styles(window.effective_mode, window.resizable),
        };
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
                // a window that was on screen a moment ago. Read it from the
                // *live* style — the saved one was captured at the first
                // borderless entry, and a window hidden since then must stay
                // hidden across a second borderless request.
                let visible = unsafe { ffi::GetWindowLongPtrW(hwnd, value::GWL_STYLE) } as u32
                    & style::VISIBLE;
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
                // The saved style's `WS_VISIBLE` bit is as it was at the first
                // borderless entry — a window hidden since then must stay
                // hidden, so the restored style carries the *live* visibility,
                // and so does the restored placement (`SetWindowPlacement`
                // re-shows according to `showCmd`).
                let live_visible = unsafe { ffi::GetWindowLongPtrW(hwnd, value::GWL_STYLE) } as u32
                    & style::VISIBLE;
                if let Some(saved) = self.window_mut(window)?.saved.take() {
                    Self::set_styles(
                        hwnd,
                        (saved.style & !style::VISIBLE) | live_visible,
                        saved.ex_style,
                    );
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
                    let mut placement = saved.placement;
                    if live_visible == 0 {
                        // A window hidden while borderless must not be re-shown
                        // by the restore: `showCmd` decides whether the
                        // placement call shows it.
                        placement.show_cmd = value::SW_HIDE as u32;
                    }
                    unsafe { ffi::SetWindowPlacement(hwnd, &raw const placement) };
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
        // procedure enforces have to be recomputed against the new one, and a
        // transparent window's chrome follows whether it now has a caption.
        let state = self.window(window)?;
        self.refresh_limits(state);
        if state.transparent {
            Self::apply_transparent_chrome(hwnd);
        }
        Ok(())
    }

    /// Asks DWM for the corners and border
    /// [`geometry::transparent_chrome`] gives a transparent window of the
    /// styles it has now.
    ///
    /// Windows 10 does not know either attribute and refuses both, which is
    /// logged and is no loss: it never rounds a window's corners, and draws no
    /// border around a frameless one.
    pub(super) fn apply_transparent_chrome(hwnd: Handle) {
        // SAFETY: reading this shell's own window's two style words.
        let styles = unsafe {
            Styles {
                style: ffi::GetWindowLongPtrW(hwnd, value::GWL_STYLE) as u32,
                ex_style: ffi::GetWindowLongPtrW(hwnd, value::GWL_EX_STYLE) as u32,
            }
        };
        let chrome = geometry::transparent_chrome(styles);
        for (attribute, wanted) in [
            (dwm::WINDOW_CORNER_PREFERENCE, chrome.corners),
            (dwm::BORDER_COLOR, chrome.border),
        ] {
            // SAFETY: `hwnd` is this shell's own window, and both attributes
            // are documented as a four-byte value: `wanted` is a live `u32`
            // read for exactly its own size.
            let result = unsafe {
                ffi::DwmSetWindowAttribute(
                    hwnd,
                    attribute,
                    (&raw const wanted).cast(),
                    size_of::<u32>() as u32,
                )
            };
            if result < 0 {
                crcbl_core::log::debug!(
                    "DwmSetWindowAttribute({attribute}, {wanted:#x}) answered {result:#x}; this \
                     Windows draws that part of a transparent window's frame its own way"
                );
            }
        }
    }

    /// Gives the window the region
    /// [`set_input_region`](crate::Shell::set_input_region) asked for, built
    /// against its frame as it is now, or removes it with `None`.
    ///
    /// A window region is what Windows hit-tests against before any window
    /// procedure is asked, so the pointer outside it goes to whatever window is
    /// beneath, in any process. It also clips what the window draws, which the
    /// seam's docs pass on to the caller. The region is in window coordinates
    /// and keeps the frame (see [`geometry::window_region`]), so it is built
    /// again whenever the window's size or frame changes.
    ///
    /// # Errors
    ///
    /// [`ShellError::Backend`] if a region could not be built or the window
    /// refused it.
    pub(super) fn apply_input_region(
        hwnd: Handle,
        region: Option<&[crate::PhysicalRect]>,
    ) -> Result<(), ShellError> {
        let Some(region) = region else {
            // SAFETY: this shell's own window; a null region removes the one
            // it has, which the system frees.
            if unsafe { ffi::SetWindowRgn(hwnd, ptr::null_mut(), 1) } == 0 {
                return Err(ShellError::Backend(format!(
                    "SetWindowRgn refused to remove window {hwnd:?}'s input region"
                )));
            }
            return Ok(());
        };
        let mut outer = Rect::default();
        let mut client = Rect::default();
        let mut origin = ffi::Point { x: 0, y: 0 };
        // SAFETY: three live, initialised out-parameters the calls write into,
        // against this shell's own window.
        let read = unsafe {
            ffi::GetWindowRect(hwnd, &raw mut outer) != 0
                && ffi::GetClientRect(hwnd, &raw mut client) != 0
                && ffi::ClientToScreen(hwnd, &raw mut origin) != 0
        };
        if !read {
            return Err(ShellError::Backend(format!(
                "could not read window {hwnd:?}'s frame to build its input region"
            )));
        }
        let rects = geometry::window_region(
            region,
            ffi::Point {
                x: origin.x - outer.left,
                y: origin.y - outer.top,
            },
            (client.width(), client.height()),
            (outer.width(), outer.height()),
        );
        // SAFETY: four integers by value; null is the system refusing.
        let combined = unsafe { ffi::CreateRectRgn(0, 0, 0, 0) };
        if combined.is_null() {
            return Err(ShellError::Backend(
                "CreateRectRgn refused: the process is out of GDI objects".to_string(),
            ));
        }
        for rect in rects {
            // SAFETY: as above. Each part is combined into the region made
            // above and freed straight after, on every path.
            let united = unsafe {
                let part = ffi::CreateRectRgn(rect.left, rect.top, rect.right, rect.bottom);
                let united = !part.is_null()
                    && ffi::CombineRgn(combined, combined, part, value::RGN_OR)
                        != value::REGION_ERROR;
                if !part.is_null() {
                    ffi::DeleteObject(part);
                }
                united
            };
            if !united {
                // SAFETY: the region made above, never handed to the window.
                unsafe { ffi::DeleteObject(combined) };
                return Err(ShellError::Backend(
                    "could not build an input region: the process is out of GDI objects"
                        .to_string(),
                ));
            }
        }
        // SAFETY: this shell's own window and a live region. On success the
        // system owns the region; on failure it is still ours and freed here.
        if unsafe { ffi::SetWindowRgn(hwnd, combined, 1) } == 0 {
            // SAFETY: as above.
            unsafe { ffi::DeleteObject(combined) };
            return Err(ShellError::Backend(format!(
                "SetWindowRgn refused window {hwnd:?}'s input region"
            )));
        }
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
    /// `WS_EX_ACCEPTFILES` is what `DragAcceptFiles` sets, so it is a fact
    /// about the window rather than a consequence of its display mode — exactly
    /// like `WS_VISIBLE` in the word next door, which the callers carry across
    /// by hand. Recomputing the extended style from [`geometry::styles`] and
    /// writing it back without this bit stops a window receiving `WM_DROPFILES`
    /// the first time it goes borderless, with nothing anywhere reporting that
    /// drops stopped working.
    pub(super) fn set_styles(hwnd: Handle, window_style: u32, ex_style: u32) {
        // SAFETY: reading this shell's own window's extended style, only to
        // carry across the write below the two bits that are state fixed
        // elsewhere rather than style: the drop registration, and the missing
        // redirection surface a transparent window was created with.
        let carried = unsafe { ffi::GetWindowLongPtrW(hwnd, value::GWL_EX_STYLE) } as u32
            & (style::EX_ACCEPT_FILES | style::EX_NO_REDIRECTION_BITMAP);
        // SAFETY: setting this shell's own window's styles. The values are
        // widened through `u32` on purpose — the style words are 32 bits even
        // on 64-bit Windows, and sign-extending them through `isize` would set
        // every high bit for a style with `WS_POPUP` in it.
        unsafe {
            ffi::SetWindowLongPtrW(hwnd, value::GWL_STYLE, window_style as isize);
            ffi::SetWindowLongPtrW(hwnd, value::GWL_EX_STYLE, (ex_style | carried) as isize);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A test binary embeds no icon, so a program without one keeps the
    /// system's default rather than failing to register its class.
    #[test]
    fn an_executable_without_an_icon_leaves_the_class_icon_null() {
        // SAFETY: a null name asks for the executable's own module.
        let exe = unsafe { ffi::GetModuleHandleW(ptr::null()) };
        assert_eq!(first_icon_group(exe), None);
        assert!(module_icon(exe, ffi::value::SM_CX_ICON).is_null());
    }

    /// `shell32.dll`, mapped for its resources alone, stands in for an
    /// executable that embeds icons: its first group is id 1, the icon its
    /// file shows, and that group loads at both of the class's sizes.
    #[test]
    fn a_module_with_icons_gives_its_first_group_at_both_sizes() {
        let name = ffi::wide("shell32.dll").expect("a literal");
        // SAFETY: `name` is NUL-terminated and outlives the call, and the
        // flags map the file for its resources without running any of it.
        let module = unsafe {
            ffi::LoadLibraryExW(
                name.as_ptr(),
                ptr::null_mut(),
                ffi::value::LOAD_AS_RESOURCES,
            )
        };
        assert!(!module.is_null(), "shell32.dll maps as a resource file");

        assert_eq!(first_icon_group(module), Some(ResourceName::Id(1)));
        for metric in [ffi::value::SM_CX_ICON, ffi::value::SM_CX_SM_ICON] {
            let icon = module_icon(module, metric);
            assert!(!icon.is_null(), "no icon at metric {metric}");
            // SAFETY: `icon` was just loaded without `LR_SHARED`, so this
            // test owns it.
            unsafe { ffi::DestroyIcon(icon) };
        }
        // SAFETY: `module` was loaded above and nothing borrows from it now.
        unsafe { ffi::FreeLibrary(module) };
    }
}
