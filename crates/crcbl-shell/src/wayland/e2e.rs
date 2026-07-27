//! Test-only scaffolding: map a window's surface so a compositor will manage
//! it.
//!
//! **Compiled only with the `wayland-e2e` feature**, which nothing but
//! `tests/run-wayland-e2e.sh` and the CI job it drives turns on.
//!
//! # Why this has to exist, and why it is not in the shell
//!
//! This is the most important thing P0.5a learned from a real compositor. On
//! Wayland a surface is **mapped exactly while it has a buffer**, and an
//! unmapped `xdg_toplevel`:
//!
//! * gets exactly one `xdg_surface.configure`, whose `xdg_toplevel.configure`
//!   carries `0 × 0` — "you choose";
//! * is not in the window manager's tree, so `swaymsg [app_id=…] resize` and
//!   `swaymsg [app_id=…] kill` match nothing at all;
//! * does not get a new configure for `set_fullscreen`, because the compositor
//!   has nothing to place.
//!
//! So *every* interesting configure — a real geometry, a fullscreen size, a
//! user resize, a close request — is downstream of attaching a buffer. And
//! attaching a buffer is **the renderer's job**: at P1 `crcbl-vk`'s swapchain
//! presents, which attaches, which maps. `docs/plan/15-windowing.md` puts
//! presentation in the HAL, so a shell that carried a `wl_shm` pool around
//! would own a second, worse presentation path forever.
//!
//! The resolution is this module: the smallest possible stand-in for the
//! swapchain — one zero-filled `wl_shm` buffer — behind a feature flag, so the
//! end-to-end suite can drive the *whole* lifecycle a year before the renderer
//! exists. It is deleted when P1's swapchain can do it for real.
//!
//! It deliberately goes through the public seam: it takes a `&dyn Shell` and
//! reads the handles out of [`SurfaceTarget`], exactly as `crcbl-vk` will. If
//! that is not enough information to present, the seam is wrong, and this is
//! the cheapest possible check of that.

use core::ffi::{c_char, c_int, c_uint, c_void};
use core::ptr;

use crcbl_core::SurfaceTarget;

use super::ffi::{self, WlArgument, WlDisplay, WlMessage, WlProxy};
use super::protocol::wayland::{wl_registry, wl_shm, wl_shm_pool, wl_surface};
use crate::{Shell, ShellError, WindowId};

unsafe extern "C" {
    fn memfd_create(name: *const c_char, flags: c_uint) -> c_int;
    fn ftruncate(fd: c_int, length: i64) -> c_int;
    fn close(fd: c_int) -> c_int;
}

/// State the registry dispatcher below fills in.
struct ShmBinding {
    registry: *mut WlProxy,
    shm: *mut WlProxy,
}

/// Binds `wl_shm` when the registry advertises it, and ignores everything else.
///
/// A private registry rather than one bound by the shell: `wl_shm` is not a
/// global the window lifecycle needs, and adding it to the backend so that a
/// test could reach it would put shared-memory presentation into the shipping
/// code path.
unsafe extern "C" fn bind_shm(
    user_data: *const c_void,
    _target: *mut c_void,
    opcode: u32,
    _message: *const WlMessage,
    args: *mut WlArgument,
) -> c_int {
    // SAFETY: `user_data` is the `*mut ShmBinding` passed to
    // `wl_proxy_add_dispatcher` below, which lives on the stack of `map_window`
    // for strictly longer than the roundtrip that can call this.
    let state = unsafe { &mut *user_data.cast::<ShmBinding>().cast_mut() };
    // SAFETY: this dispatcher is attached only to a `wl_registry`, so `args`
    // matches that interface's events for `opcode`.
    let event = unsafe { wl_registry::decode_event(opcode, args.cast_const()) };
    if let Some(wl_registry::Event::Global {
        name,
        interface,
        version,
    }) = event
        && interface == wl_shm::NAME
        && state.shm.is_null()
    {
        // SAFETY: the registry is live, and binding from inside a dispatch is
        // ordinary libwayland usage — the request is queued, not reentrant.
        state.shm =
            unsafe { wl_registry::bind(state.registry, name, &wl_shm::INTERFACE, version.min(1)) };
    }
    0
}

/// Swallows the events of objects this module creates but does not read.
///
/// `wl_shm.format` and `wl_buffer.release` both arrive; a proxy with no
/// dispatcher at all makes libwayland log a warning per event, which buries the
/// compositor log the harness prints on failure.
unsafe extern "C" fn ignore(
    _user_data: *const c_void,
    _target: *mut c_void,
    _opcode: u32,
    _message: *const WlMessage,
    _args: *mut WlArgument,
) -> c_int {
    0
}

/// Attaches a zero-filled buffer to `window`'s surface, so the compositor maps
/// it.
///
/// Call it *after* the first configure, which is when `xdg-shell` first permits
/// a buffer. The next `pump` will see the compositor's real geometry.
///
/// # Errors
///
/// [`ShellError::InvalidWindow`] for a stale handle, or
/// [`ShellError::Backend`] if the compositor has no `wl_shm` or the anonymous
/// file could not be created.
///
/// # Panics
///
/// Never; every fallible step is reported as an error.
pub fn map_window(shell: &dyn Shell, window: WindowId) -> Result<(), ShellError> {
    let SurfaceTarget::Wayland { display, surface } = shell.surface_target(window)? else {
        return Err(ShellError::Backend(
            "not a Wayland window; this helper is only for the Wayland backend".to_string(),
        ));
    };
    let size = shell
        .window_state(window)?
        .size()
        .ok_or_else(|| ShellError::Backend("map_window before the first configure".to_string()))?;

    let lib = ffi::load().map_err(|detail| ShellError::Backend(detail.to_string()))?;
    let display: *mut WlDisplay = display.as_ptr().cast();
    let surface: *mut WlProxy = surface.as_ptr().cast();

    let mut binding = ShmBinding {
        registry: ptr::null_mut(),
        shm: ptr::null_mut(),
    };
    // SAFETY: the display belongs to the shell and is live for as long as the
    // shell is borrowed. The registry, the roundtrip and the dispatcher all run
    // on this thread, and `binding` outlives every call that can reach it.
    unsafe {
        binding.registry =
            super::protocol::wayland::wl_display::get_registry(ffi::display_as_proxy(display));
        if binding.registry.is_null() {
            return Err(ShellError::Backend("wl_display.get_registry".to_string()));
        }
        (lib.proxy_add_dispatcher)(
            binding.registry,
            bind_shm,
            ptr::from_mut(&mut binding).cast(),
            ptr::null_mut(),
        );
        // Two round trips: the first delivers the globals, the second the
        // `wl_shm.format` events that follow the bind.
        if (lib.display_roundtrip)(display) < 0 || (lib.display_roundtrip)(display) < 0 {
            return Err(ShellError::Backend("roundtrip failed".to_string()));
        }
    }
    if binding.shm.is_null() {
        return Err(ShellError::Backend(
            "the compositor does not advertise wl_shm".to_string(),
        ));
    }
    // SAFETY: `binding.shm` was just created on this connection.
    unsafe { (lib.proxy_add_dispatcher)(binding.shm, ignore, ptr::null(), ptr::null_mut()) };

    let stride = i64::from(size.width) * 4;
    let length = stride * i64::from(size.height);
    // SAFETY: the name is a NUL-terminated literal; `memfd_create` returns a
    // new descriptor or -1, and takes no ownership of anything.
    let fd = unsafe { memfd_create(c"crcbl-e2e".as_ptr(), 0) };
    if fd < 0 {
        return Err(ShellError::Backend("memfd_create failed".to_string()));
    }
    // SAFETY: `fd` is the descriptor just returned, and `length` is positive.
    if unsafe { ftruncate(fd, length) } != 0 {
        // SAFETY: closing the descriptor we just opened, once.
        unsafe { close(fd) };
        return Err(ShellError::Backend("ftruncate failed".to_string()));
    }

    // SAFETY: every proxy is live on this connection, `fd` names a file of
    // exactly `length` bytes, and the buffer geometry matches it — which is
    // what `wl_shm` validates and disconnects the client over. The pool and
    // buffer are intentionally not destroyed: this helper's connection is torn
    // down by the end of the test that called it.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the sizes come from a compositor configure and are far below i32::MAX"
    )]
    unsafe {
        let pool = wl_shm::create_pool(binding.shm, fd, length as i32);
        (lib.proxy_add_dispatcher)(pool, ignore, ptr::null(), ptr::null_mut());
        let buffer = wl_shm_pool::create_buffer(
            pool,
            0,
            size.width as i32,
            size.height as i32,
            stride as i32,
            wl_shm::format::ARGB8888,
        );
        (lib.proxy_add_dispatcher)(buffer, ignore, ptr::null(), ptr::null_mut());
        wl_surface::attach(surface, buffer, 0, 0);
        wl_surface::damage(surface, 0, 0, size.width as i32, size.height as i32);
        wl_surface::commit(surface);
        (lib.display_flush)(display);
        close(fd);
    }
    Ok(())
}
