//! Test-only scaffolding: map a window's surface so a compositor will manage
//! it, and give its seat real input devices.
//!
//! **Compiled only with the `wayland-e2e` feature**, which nothing but
//! `tests/run-wayland-e2e.sh` and the CI job it drives turns on.
//!
//! # Why there is a virtual keyboard and pointer in here
//!
//! A wlroots **headless** compositor has no input devices, so its `wl_seat`
//! advertises `capabilities = 0` and a client bound to it receives no key, no
//! button and no motion, ever. Driving sway's IPC does not help:
//! `swaymsg seat seat0 cursor move 100 100` returns `"success": true` and moves
//! nothing, because there is no cursor to move.
//!
//! [`VirtualInput`](crate::wayland_test_support::VirtualInput) creates a
//! `zwp_virtual_keyboard_v1` and a
//! `zwlr_virtual_pointer_v1` on that seat instead. Everything they send goes
//! through the compositor's **whole** input path — focus, surface hit-testing,
//! serials, XKB state, frame batching — before it comes back to the client
//! under test, which cannot tell it from a physical mouse. That is the
//! difference between testing our own plumbing and testing the thing that has
//! to work.
//!
//! It also gets the seat-capability lifecycle for free: the seat starts empty,
//! gains `pointer | keyboard` when
//! [`VirtualInput::attach`](crate::wayland_test_support::VirtualInput::attach)
//! runs, and loses
//! them again when it is dropped — the mid-session hotplug the backend has to
//! survive.
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
use super::protocol::virtual_keyboard::{zwp_virtual_keyboard_manager_v1, zwp_virtual_keyboard_v1};
use super::protocol::virtual_pointer::{zwlr_virtual_pointer_manager_v1, zwlr_virtual_pointer_v1};
use super::protocol::wayland::{
    wl_data_device, wl_data_device_manager, wl_data_source, wl_pointer, wl_registry, wl_seat,
    wl_shm, wl_shm_pool, wl_surface,
};
use crate::{PhysicalSize, Shell, ShellError, WindowId};

unsafe extern "C" {
    fn memfd_create(name: *const c_char, flags: c_uint) -> c_int;
    fn ftruncate(fd: c_int, length: i64) -> c_int;
    fn write(fd: c_int, buf: *const c_void, count: usize) -> isize;
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
    // `wl_proxy_add_dispatcher` in `map_window`. It points into a `Box` that
    // outlives the registry proxy — which `map_window` destroys before it
    // returns, so no later `global`/`global_remove` can reach a freed
    // allocation. It used to point at `map_window`'s own stack frame, and an
    // output hotplug after that frame was gone re-entered here through it.
    let state = unsafe { &mut *user_data.cast::<ShmBinding>().cast_mut() };
    // SAFETY: this dispatcher is attached only to a `wl_registry`, so `args`
    // matches that interface's events for `opcode`.
    let event = unsafe { wl_registry::decode_event(opcode, &*args.cast_const()) };
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

    // Boxed, not a local: the pointer goes to `wl_proxy_add_dispatcher` and
    // libwayland keeps it for as long as the registry proxy lives. A local left
    // `bind_shm` holding a pointer into a dead stack frame, which any later
    // `wl_registry.global`/`global_remove` — an output hotplug, which this
    // suite provokes deliberately — would have followed. `VirtualInput::attach`
    // and `DragSource::attach` box theirs for the same reason.
    let mut binding = Box::new(ShmBinding {
        registry: ptr::null_mut(),
        shm: ptr::null_mut(),
    });
    // SAFETY: the display belongs to the shell and is live for as long as the
    // shell is borrowed. The registry, the roundtrip and the dispatcher all run
    // on this thread, and the boxed `binding` outlives the registry proxy.
    unsafe {
        binding.registry =
            super::protocol::wayland::wl_display::get_registry(ffi::display_as_proxy(display));
        if binding.registry.is_null() {
            return Err(ShellError::Backend("wl_display.get_registry".to_string()));
        }
        (lib.proxy_add_dispatcher)(
            binding.registry,
            bind_shm,
            ptr::from_mut(binding.as_mut()).cast(),
            ptr::null_mut(),
        );
        // Two round trips: the first delivers the globals, the second the
        // `wl_shm.format` events that follow the bind.
        if (lib.display_roundtrip)(display) < 0 || (lib.display_roundtrip)(display) < 0 {
            (lib.proxy_destroy)(binding.registry);
            return Err(ShellError::Backend("roundtrip failed".to_string()));
        }
    }
    let shm = binding.shm;
    // The private registry has done its one job. Destroying it here — rather
    // than leaking one per call, as this used to — is also what makes
    // `bind_shm` unreachable from now on.
    // SAFETY: the registry was created on this connection above and nothing
    // else refers to it. `wl_registry` has no protocol destructor, so a
    // client-side destroy is the whole of it.
    unsafe { (lib.proxy_destroy)(binding.registry) };
    if shm.is_null() {
        return Err(ShellError::Backend(
            "the compositor does not advertise wl_shm".to_string(),
        ));
    }
    // SAFETY: `shm` was just bound on this connection.
    unsafe { (lib.proxy_add_dispatcher)(shm, ignore, ptr::null(), ptr::null_mut()) };
    let result = attach_shm_buffer(lib, display, surface, size, shm);
    // SAFETY: `shm` is live and unused from here; the pool created from it is a
    // separate object that deliberately outlives it. `wl_shm.release` only
    // exists at version 2 and this binds version 1, so a client-side destroy is
    // the only way to say it.
    unsafe { (lib.proxy_destroy)(shm) };
    result
}

/// Wraps an anonymous file in a `wl_buffer` and attaches it to `surface`.
///
/// Split out of [`map_window`] so the private registry and `wl_shm` have
/// exactly one place to be destroyed, whichever step fails.
fn attach_shm_buffer(
    lib: &'static ffi::Lib,
    display: *mut WlDisplay,
    surface: *mut WlProxy,
    size: crate::PhysicalSize,
    shm: *mut WlProxy,
) -> Result<(), ShellError> {
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
        let pool = wl_shm::create_pool(shm, fd, length as i32);
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

// ---------------------------------------------------------------------------
// Virtual input devices
// ---------------------------------------------------------------------------

/// The XKB keymap [`VirtualInput`] uploads.
///
/// **Hand-written and self-contained on purpose.** Two properties matter:
///
/// * **Determinism.** A test that asserts `KeyW` produces the keysym `w` must
///   not depend on the developer's or the runner's configured layout. This
///   keymap always says `w`.
/// * **No `include`.** The usual `include "complete"` in `xkb_types` and
///   `xkb_compat` reads `xkeyboard-config`'s data files off disk, so a runner
///   without that package would fail in a way that looks like our bug. Every
///   key type, interpretation and modifier map this suite needs is spelled out
///   here.
///
/// It covers a deliberately small keyboard: enough letters to type with, both
/// Shift keys, Control, Alt, Caps Lock, Space, Return, Backspace and an arrow.
/// Modifier keys carry `repeat = False`, which is what makes the key-repeat
/// test meaningful — a keymap where everything repeats would prove nothing
/// about the `xkb_keymap_key_repeats` guard.
///
/// Verified to compile with `xkbcli compile-keymap --keymap`.
pub const TEST_KEYMAP: &str = include_str!("e2e-keymap.xkb");

/// Globals the virtual-input registry below binds.
struct InputGlobals {
    registry: *mut WlProxy,
    seat: *mut WlProxy,
    keyboard_manager: *mut WlProxy,
    pointer_manager: *mut WlProxy,
}

/// Binds `wl_seat` and the two virtual-device managers, ignoring everything
/// else.
///
/// A private registry rather than the shell's: the shell must not know that
/// virtual input exists, and binding a second `wl_seat` on the same connection
/// is ordinary Wayland.
unsafe extern "C" fn bind_input(
    user_data: *const c_void,
    _target: *mut c_void,
    opcode: u32,
    _message: *const WlMessage,
    args: *mut WlArgument,
) -> c_int {
    // SAFETY: `user_data` is the `*mut InputGlobals` passed to
    // `wl_proxy_add_dispatcher` in `VirtualInput::attach`, which lives for
    // strictly longer than the roundtrip that can call this.
    let state = unsafe { &mut *user_data.cast::<InputGlobals>().cast_mut() };
    // SAFETY: this dispatcher is attached only to a `wl_registry`, so `args`
    // matches that interface's events for `opcode`.
    let event = unsafe { wl_registry::decode_event(opcode, &*args.cast_const()) };
    let Some(wl_registry::Event::Global {
        name,
        interface,
        version,
    }) = event
    else {
        return 0;
    };
    // SAFETY: the registry is live, and binding from inside a dispatch is
    // ordinary libwayland usage — the request is queued, not reentrant.
    unsafe {
        if interface == wl_seat::NAME && state.seat.is_null() {
            state.seat = wl_registry::bind(state.registry, name, &wl_seat::INTERFACE, 1);
        } else if interface == zwp_virtual_keyboard_manager_v1::NAME
            && state.keyboard_manager.is_null()
        {
            state.keyboard_manager = wl_registry::bind(
                state.registry,
                name,
                &zwp_virtual_keyboard_manager_v1::INTERFACE,
                1,
            );
        } else if interface == zwlr_virtual_pointer_manager_v1::NAME
            && state.pointer_manager.is_null()
        {
            state.pointer_manager = wl_registry::bind(
                state.registry,
                name,
                &zwlr_virtual_pointer_manager_v1::INTERFACE,
                version.min(2),
            );
        }
    }
    0
}

/// A keyboard and a pointer plugged into the compositor's seat.
///
/// Dropping it unplugs them, which is itself a test: the seat's capabilities go
/// back to zero and the backend has to tear its `wl_pointer` and `wl_keyboard`
/// down without leaving a window focused by a device that no longer exists.
///
/// Every method flushes, so the compositor sees the event before the caller
/// pumps for it. None of them block: the answer comes back through the shell's
/// own event stream, which is the point.
pub struct VirtualInput {
    lib: &'static ffi::Lib,
    display: *mut WlDisplay,
    globals: Box<InputGlobals>,
    keyboard: *mut WlProxy,
    pointer: *mut WlProxy,
    /// Real-modifier masks, in [`TEST_KEYMAP`]'s numbering.
    ///
    /// **The compositor does not track these for a virtual keyboard.** wlroots
    /// builds its `wlr_keyboard_key_event` with `update_state = false`, because
    /// `zwp_virtual_keyboard_v1` makes the *client* responsible for saying what
    /// the modifier state is — the protocol has a `modifiers` request for
    /// exactly that. A harness that only sent `key` would press Shift and see
    /// the compositor's XKB state never move, so every shifted keysym would
    /// come back unshifted. Emulating the driver here is what makes the
    /// modifier assertions test *our* backend rather than a hole in the
    /// harness.
    depressed: core::cell::Cell<u32>,
    locked: core::cell::Cell<u32>,
}

impl core::fmt::Debug for VirtualInput {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("VirtualInput")
            .field("keyboard", &!self.keyboard.is_null())
            .field("pointer", &!self.pointer.is_null())
            .finish()
    }
}

impl VirtualInput {
    /// Plugs a keyboard and a pointer into the seat of `shell`'s compositor.
    ///
    /// # Errors
    ///
    /// [`ShellError::Backend`] if the compositor advertises no `wl_seat` or
    /// none of the virtual-device protocols, or if the keymap could not be put
    /// in an anonymous file.
    ///
    /// # Panics
    ///
    /// Never; every fallible step is reported as an error.
    pub fn attach(shell: &dyn Shell, window: WindowId) -> Result<Self, ShellError> {
        let SurfaceTarget::Wayland { display, .. } = shell.surface_target(window)? else {
            return Err(ShellError::Backend(
                "not a Wayland window; this helper is only for the Wayland backend".to_string(),
            ));
        };
        let lib = ffi::load().map_err(|detail| ShellError::Backend(detail.to_string()))?;
        let display: *mut WlDisplay = display.as_ptr().cast();

        let mut globals = Box::new(InputGlobals {
            registry: ptr::null_mut(),
            seat: ptr::null_mut(),
            keyboard_manager: ptr::null_mut(),
            pointer_manager: ptr::null_mut(),
        });
        // SAFETY: the display belongs to the shell and is live for as long as
        // the shell is borrowed. `globals` is boxed, so the pointer handed to
        // the dispatcher stays valid while this value lives, and the dispatcher
        // only runs inside the round trips below.
        unsafe {
            globals.registry =
                super::protocol::wayland::wl_display::get_registry(ffi::display_as_proxy(display));
            if globals.registry.is_null() {
                return Err(ShellError::Backend("wl_display.get_registry".to_string()));
            }
            (lib.proxy_add_dispatcher)(
                globals.registry,
                bind_input,
                ptr::from_mut(globals.as_mut()).cast(),
                ptr::null_mut(),
            );
            if (lib.display_roundtrip)(display) < 0 {
                return Err(ShellError::Backend("roundtrip failed".to_string()));
            }
        }
        if globals.seat.is_null() {
            return Err(ShellError::Backend(
                "the compositor has no wl_seat".to_string(),
            ));
        }
        // SAFETY: the seat is live; `wl_seat`'s own events are not read here.
        unsafe { (lib.proxy_add_dispatcher)(globals.seat, ignore, ptr::null(), ptr::null_mut()) };

        let mut input = Self {
            lib,
            display,
            globals,
            keyboard: ptr::null_mut(),
            pointer: ptr::null_mut(),
            depressed: core::cell::Cell::new(0),
            locked: core::cell::Cell::new(0),
        };
        input.attach_keyboard()?;
        input.attach_pointer()?;
        // SAFETY: the display is live; this lets the compositor process both
        // device creations before the caller starts sending events.
        unsafe { (lib.display_roundtrip)(display) };
        Ok(input)
    }

    fn attach_keyboard(&mut self) -> Result<(), ShellError> {
        if self.globals.keyboard_manager.is_null() {
            return Err(ShellError::Backend(
                "the compositor has no zwp_virtual_keyboard_manager_v1".to_string(),
            ));
        }
        // wlroots refuses a `key` before a keymap, so this has to come first.
        // The trailing NUL is counted: the compositor mmaps `size` bytes and
        // hands them to `xkb_keymap_new_from_string`, which needs one.
        let bytes = TEST_KEYMAP.as_bytes();
        let size = bytes.len() + 1;
        // SAFETY: the name is a NUL-terminated literal, and `memfd_create`
        // either returns a fresh descriptor or -1.
        let fd = unsafe { memfd_create(c"crcbl-e2e-keymap".as_ptr(), 0) };
        if fd < 0 {
            return Err(ShellError::Backend("memfd_create failed".to_string()));
        }
        // SAFETY: `fd` is the descriptor just opened, `bytes` is a live slice of
        // exactly the length passed, and the extra NUL is written from a
        // one-byte local. `write` may return short, which is checked.
        let written = unsafe {
            let head = write(fd, bytes.as_ptr().cast(), bytes.len());
            let nul = 0u8;
            let tail = write(fd, ptr::from_ref(&nul).cast(), 1);
            head.max(0) + tail.max(0)
        };
        if usize::try_from(written).unwrap_or(0) != size {
            // SAFETY: closing the descriptor we just opened, once.
            unsafe { close(fd) };
            return Err(ShellError::Backend("short write of the keymap".to_string()));
        }
        // SAFETY: the manager and seat are live; `keymap` takes ownership of
        // nothing — the descriptor is duplicated across the socket — so it is
        // closed here afterwards.
        unsafe {
            self.keyboard = zwp_virtual_keyboard_manager_v1::create_virtual_keyboard(
                self.globals.keyboard_manager,
                self.globals.seat,
            );
            (self.lib.proxy_add_dispatcher)(self.keyboard, ignore, ptr::null(), ptr::null_mut());
            zwp_virtual_keyboard_v1::keymap(
                self.keyboard,
                1, // WL_KEYBOARD_KEYMAP_FORMAT_XKB_V1
                fd,
                u32::try_from(size).unwrap_or(u32::MAX),
            );
            (self.lib.display_flush)(self.display);
            close(fd);
        }
        Ok(())
    }

    fn attach_pointer(&mut self) -> Result<(), ShellError> {
        if self.globals.pointer_manager.is_null() {
            return Err(ShellError::Backend(
                "the compositor has no zwlr_virtual_pointer_manager_v1".to_string(),
            ));
        }
        // SAFETY: the manager and seat are live on this connection.
        unsafe {
            self.pointer = zwlr_virtual_pointer_manager_v1::create_virtual_pointer(
                self.globals.pointer_manager,
                self.globals.seat,
            );
            (self.lib.proxy_add_dispatcher)(self.pointer, ignore, ptr::null(), ptr::null_mut());
            (self.lib.display_flush)(self.display);
        }
        Ok(())
    }

    /// A plausible, monotonically increasing event time in milliseconds.
    fn now(&self) -> u32 {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the compositor's own timestamps are 32-bit milliseconds too"
        )]
        let millis = (ffi::monotonic_nanos() / 1_000_000) as u32;
        millis
    }

    fn flush(&self) {
        // SAFETY: the display is live for as long as the shell that owns it.
        unsafe { (self.lib.display_flush)(self.display) };
    }

    /// Presses or releases a key, by **evdev** code — the same numbering
    /// `wl_keyboard.key` reports back.
    ///
    /// Sends the matching `modifiers` request when the key is one
    /// [`TEST_KEYMAP`] puts in a modifier map, because the compositor does not
    /// track them for a virtual keyboard — see this type's own docs.
    pub fn key(&self, evdev: u32, pressed: bool) {
        // Real-modifier bit positions, in the order `TEST_KEYMAP`'s
        // `modifier_map` statements assign them: Shift, Lock, Control, Mod1.
        let bit = match evdev {
            42 | 54 => Some(1 << 0), // KEY_LEFTSHIFT, KEY_RIGHTSHIFT
            29 => Some(1 << 2),      // KEY_LEFTCTRL
            56 => Some(1 << 3),      // KEY_LEFTALT
            _ => None,
        };
        if let Some(bit) = bit {
            let mask = self.depressed.get();
            self.depressed
                .set(if pressed { mask | bit } else { mask & !bit });
        } else if evdev == 58 && pressed {
            // KEY_CAPSLOCK is a *lock*: it toggles on press and stays.
            self.locked.set(self.locked.get() ^ (1 << 1));
        }
        // SAFETY: the virtual keyboard is live and has a keymap, which is what
        // wlroots checks before accepting a key.
        unsafe {
            zwp_virtual_keyboard_v1::key(self.keyboard, self.now(), evdev, u32::from(pressed));
            zwp_virtual_keyboard_v1::modifiers(
                self.keyboard,
                self.depressed.get(),
                0,
                self.locked.get(),
                0,
            );
        }
        self.flush();
    }

    /// Presses and releases a key.
    pub fn tap(&self, evdev: u32) {
        self.key(evdev, true);
        self.key(evdev, false);
    }

    /// Moves the pointer to an absolute position within an extent.
    ///
    /// The extent is the output's size, because that is the space
    /// `zwlr_virtual_pointer_v1.motion_absolute` is defined in — the compositor
    /// then decides which surface the position lands on, exactly as it would
    /// for a real device.
    pub fn move_to(&self, x: u32, y: u32, extent: PhysicalSize) {
        // SAFETY: the virtual pointer is live; every argument is a plain
        // integer, and `frame` is what makes the compositor act on the batch.
        unsafe {
            zwlr_virtual_pointer_v1::motion_absolute(
                self.pointer,
                self.now(),
                x,
                y,
                extent.width,
                extent.height,
            );
            zwlr_virtual_pointer_v1::frame(self.pointer);
        }
        self.flush();
    }

    /// Moves the pointer by a relative amount, which is what produces
    /// unaccelerated `relative_motion`.
    pub fn move_by(&self, dx: f64, dy: f64) {
        // SAFETY: the virtual pointer is live; the deltas are `wl_fixed`.
        unsafe {
            zwlr_virtual_pointer_v1::motion(self.pointer, self.now(), to_fixed(dx), to_fixed(dy));
            zwlr_virtual_pointer_v1::frame(self.pointer);
        }
        self.flush();
    }

    /// Presses or releases a pointer button, by **evdev** code (`BTN_LEFT` is
    /// `0x110`).
    pub fn button(&self, evdev: u32, pressed: bool) {
        // SAFETY: the virtual pointer is live.
        unsafe {
            zwlr_virtual_pointer_v1::button(self.pointer, self.now(), evdev, u32::from(pressed));
            zwlr_virtual_pointer_v1::frame(self.pointer);
        }
        self.flush();
    }

    /// Scrolls `pixels` of continuous, finger-driven motion on the vertical
    /// axis — a touchpad, not a wheel.
    ///
    /// No `axis_discrete`, because a touchpad has no detents. That is the
    /// difference [`ScrollDelta`](crate::ScrollDelta) refuses to collapse, and
    /// this is the half of it a wheel cannot produce.
    pub fn touchpad_scroll(&self, pixels: f64) {
        let time = self.now();
        // SAFETY: the virtual pointer is live; axis 0 is
        // `wl_pointer.axis.vertical_scroll` and source 1 is `finger`.
        unsafe {
            zwlr_virtual_pointer_v1::axis_source(self.pointer, 1);
            zwlr_virtual_pointer_v1::axis(self.pointer, time, 0, to_fixed(pixels));
            zwlr_virtual_pointer_v1::frame(self.pointer);
        }
        self.flush();
    }

    /// Scrolls `detents` notches of a real wheel on the vertical axis.
    ///
    /// Sends the discrete count *and* the continuous equivalent, which is what
    /// a notched wheel does — and is what makes the "detents, not pixels"
    /// assertion meaningful.
    pub fn wheel(&self, detents: i32) {
        let time = self.now();
        // 15 units is one notch by the convention libinput uses for a wheel.
        let value = to_fixed(f64::from(detents) * 15.0);
        // SAFETY: the virtual pointer is live; the axis is
        // `wl_pointer.axis.vertical_scroll`.
        unsafe {
            zwlr_virtual_pointer_v1::axis_source(self.pointer, 0);
            zwlr_virtual_pointer_v1::axis_discrete(self.pointer, time, 0, value, detents);
            zwlr_virtual_pointer_v1::frame(self.pointer);
        }
        self.flush();
    }
}

impl Drop for VirtualInput {
    fn drop(&mut self) {
        // SAFETY: every proxy was created on this connection and each
        // destructor belongs to its own interface. Destroying the devices is
        // what makes the seat drop back to zero capabilities, which the
        // hotplug test depends on.
        unsafe {
            if !self.pointer.is_null() {
                zwlr_virtual_pointer_v1::destroy(self.pointer);
            }
            if !self.keyboard.is_null() {
                zwp_virtual_keyboard_v1::destroy(self.keyboard);
            }
            if !self.globals.seat.is_null() {
                (self.lib.proxy_destroy)(self.globals.seat);
            }
            if !self.globals.keyboard_manager.is_null() {
                (self.lib.proxy_destroy)(self.globals.keyboard_manager);
            }
            if !self.globals.pointer_manager.is_null() {
                zwlr_virtual_pointer_manager_v1::destroy(self.globals.pointer_manager);
            }
            (self.lib.proxy_destroy)(self.globals.registry);
            (self.lib.display_flush)(self.display);
        }
    }
}

// ---------------------------------------------------------------------------
// A drag origin
// ---------------------------------------------------------------------------

/// Starts a drag-and-drop from one of the shell's own surfaces.
///
/// # Why the drag *source* is scaffolding rather than shell
///
/// P0.5c implements the drag **destination**: a drop that arrives becomes a
/// [`ShellEvent::DroppedFile`](crate::ShellEvent::DroppedFile). Starting a drag
/// is a different feature — the editor will want it, the engine does not — and
/// it needs two things this slice deliberately does not have: a seam request to
/// trigger it, and a **drag icon**, which is a `wl_surface` with a buffer, and
/// buffers are the renderer's. Adding a half of it to [`Shell`] now would put a
/// method there that no backend can honour until P1.
///
/// But the destination cannot be tested without one, so the minimum lives here,
/// beside the stand-in buffer and the virtual devices, for the same reason and
/// under the same feature flag.
///
/// # How it gets a serial without asking the shell
///
/// `wl_data_device.start_drag` must quote the serial of an **implicit pointer
/// grab** — wlroots checks it against the seat's own last button-press serial,
/// requires the button to still be held, and requires the pointer's focus to be
/// the origin surface. The shell has that serial (it is on the
/// [`Button`](crate::ShellEvent::Button) event's seat) and the seam has no way
/// to expose it, which is exactly as it should be.
///
/// So this binds *its own* `wl_pointer` on the same connection. A compositor
/// sends a button event to **every** pointer resource the focused client holds,
/// with one serial for all of them, so watching that second pointer yields the
/// same number the shell saw — without a test-only hole in the shell.
pub struct DragSource {
    lib: &'static ffi::Lib,
    display: *mut WlDisplay,
    state: Box<DragState>,
}

/// The dispatcher's side of a [`DragSource`].
struct DragState {
    registry: *mut WlProxy,
    seat: *mut WlProxy,
    manager: *mut WlProxy,
    device: *mut WlProxy,
    pointer: *mut WlProxy,
    source: *mut WlProxy,
    /// The serial of the last `wl_pointer.button` press, which is the implicit
    /// grab a `start_drag` has to name.
    press_serial: core::cell::Cell<u32>,
    /// What a `wl_data_source.send` should answer with.
    payload: core::cell::RefCell<Vec<u8>>,
    /// Whether the compositor has asked for the payload yet.
    sent: core::cell::Cell<bool>,
    /// Whether the drag ended without a drop.
    cancelled: core::cell::Cell<bool>,
}

impl core::fmt::Debug for DragSource {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DragSource")
            .field("press_serial", &self.state.press_serial.get())
            .field("sent", &self.state.sent.get())
            .finish()
    }
}

/// Binds `wl_seat` and `wl_data_device_manager` for a [`DragSource`].
unsafe extern "C" fn bind_drag(
    user_data: *const c_void,
    _target: *mut c_void,
    opcode: u32,
    _message: *const WlMessage,
    args: *mut WlArgument,
) -> c_int {
    // SAFETY: `user_data` is the boxed `DragState` handed to
    // `wl_proxy_add_dispatcher` in `DragSource::attach`, which outlives every
    // dispatch that can reach it.
    let state = unsafe { &mut *user_data.cast::<DragState>().cast_mut() };
    // SAFETY: attached only to a `wl_registry`.
    let event = unsafe { wl_registry::decode_event(opcode, &*args.cast_const()) };
    let Some(wl_registry::Event::Global {
        name,
        interface,
        version,
    }) = event
    else {
        return 0;
    };
    // SAFETY: the registry is live, and binding from inside a dispatch is
    // ordinary libwayland usage.
    unsafe {
        if interface == wl_seat::NAME && state.seat.is_null() {
            state.seat = wl_registry::bind(state.registry, name, &wl_seat::INTERFACE, 1);
        } else if interface == wl_data_device_manager::NAME && state.manager.is_null() {
            state.manager = wl_registry::bind(
                state.registry,
                name,
                &wl_data_device_manager::INTERFACE,
                version.min(3),
            );
        }
    }
    0
}

/// Watches this connection's second `wl_pointer` for the grab serial, and
/// answers the source's `send`.
unsafe extern "C" fn drag_dispatch(
    user_data: *const c_void,
    target: *mut c_void,
    opcode: u32,
    _message: *const WlMessage,
    args: *mut WlArgument,
) -> c_int {
    // SAFETY: as `bind_drag`.
    let state = unsafe { &mut *user_data.cast::<DragState>().cast_mut() };
    // SAFETY: libwayland hands the dispatcher its closure argument array, which
    // outlives this call; the borrow pins the decoders' lifetime to it.
    let args = unsafe { &*args.cast_const() };
    if target == state.pointer.cast() {
        // SAFETY: this branch is only reached for the `wl_pointer` proxy.
        if let Some(wl_pointer::Event::Button {
            serial,
            state: down,
            ..
        }) = unsafe { wl_pointer::decode_event(opcode, args) }
            && down == wl_pointer::button_state::PRESSED
        {
            state.press_serial.set(serial);
        }
        return 0;
    }
    if target == state.source.cast() {
        // SAFETY: this branch is only reached for the `wl_data_source` proxy.
        match unsafe { wl_data_source::decode_event(opcode, args) } {
            Some(wl_data_source::Event::Send { fd, .. }) => {
                let payload = state.payload.borrow();
                // A blocking write, deliberately: the harness's payload is a
                // handful of bytes, far below the 64 KiB a pipe holds, so it
                // cannot block — and the reader is the shell under test, on
                // this very thread, which is precisely why a *large* payload
                // would have to be written the way `fd::Writing` does.
                // SAFETY: `fd` is the descriptor the compositor handed over,
                // owned by this process and closed exactly once below.
                unsafe {
                    write(fd, payload.as_ptr().cast(), payload.len());
                    close(fd);
                }
                state.sent.set(true);
            }
            Some(wl_data_source::Event::Cancelled) => state.cancelled.set(true),
            _ => {}
        }
    }
    0
}

impl DragSource {
    /// Prepares a drag origin on the seat of `shell`'s compositor.
    ///
    /// Attach it **after** [`VirtualInput`], which is what gives the seat a
    /// pointer at all.
    ///
    /// # Errors
    ///
    /// [`ShellError::Backend`] if the compositor advertises no `wl_seat` or no
    /// `wl_data_device_manager`.
    ///
    /// # Panics
    ///
    /// Never; every fallible step is reported as an error.
    pub fn attach(shell: &dyn Shell, window: WindowId) -> Result<Self, ShellError> {
        let SurfaceTarget::Wayland { display, .. } = shell.surface_target(window)? else {
            return Err(ShellError::Backend(
                "not a Wayland window; this helper is only for the Wayland backend".to_string(),
            ));
        };
        let lib = ffi::load().map_err(|detail| ShellError::Backend(detail.to_string()))?;
        let display: *mut WlDisplay = display.as_ptr().cast();

        let mut state = Box::new(DragState {
            registry: ptr::null_mut(),
            seat: ptr::null_mut(),
            manager: ptr::null_mut(),
            device: ptr::null_mut(),
            pointer: ptr::null_mut(),
            source: ptr::null_mut(),
            press_serial: core::cell::Cell::new(0),
            payload: core::cell::RefCell::new(Vec::new()),
            sent: core::cell::Cell::new(false),
            cancelled: core::cell::Cell::new(false),
        });
        // SAFETY: the display belongs to the shell and is live while it is
        // borrowed; `state` is boxed, so the pointer handed to the dispatcher
        // stays valid for this value's lifetime.
        unsafe {
            state.registry =
                super::protocol::wayland::wl_display::get_registry(ffi::display_as_proxy(display));
            if state.registry.is_null() {
                return Err(ShellError::Backend("wl_display.get_registry".to_string()));
            }
            (lib.proxy_add_dispatcher)(
                state.registry,
                bind_drag,
                ptr::from_mut(state.as_mut()).cast(),
                ptr::null_mut(),
            );
            if (lib.display_roundtrip)(display) < 0 {
                return Err(ShellError::Backend("roundtrip failed".to_string()));
            }
        }
        if state.seat.is_null() || state.manager.is_null() {
            return Err(ShellError::Backend(
                "the compositor has no wl_seat or no wl_data_device_manager".to_string(),
            ));
        }
        // SAFETY: every proxy below is created live on this connection, and the
        // dispatcher outlives all of them.
        unsafe {
            (lib.proxy_add_dispatcher)(state.seat, ignore, ptr::null(), ptr::null_mut());
            state.pointer = wl_seat::get_pointer(state.seat);
            state.device = wl_data_device_manager::get_data_device(state.manager, state.seat);
            let data = ptr::from_mut(state.as_mut()).cast::<c_void>();
            (lib.proxy_add_dispatcher)(state.pointer, drag_dispatch, data, ptr::null_mut());
            // This second data device would otherwise receive the drag's own
            // enter/leave events with no listener, which libwayland logs.
            (lib.proxy_add_dispatcher)(state.device, ignore, ptr::null(), ptr::null_mut());
            (lib.display_roundtrip)(display);
        }
        Ok(Self {
            lib,
            display,
            state,
        })
    }

    /// The serial of the last button press the compositor sent this connection.
    ///
    /// Zero until one has happened, which is itself the assertion a test wants:
    /// a drag cannot be started without one.
    #[must_use]
    pub fn press_serial(&self) -> u32 {
        self.state.press_serial.get()
    }

    /// Whether the compositor has asked for the payload.
    #[must_use]
    pub fn was_asked_for_data(&self) -> bool {
        self.state.sent.get()
    }

    /// Whether the drag ended with no drop.
    #[must_use]
    pub fn was_cancelled(&self) -> bool {
        self.state.cancelled.get()
    }

    /// Starts dragging `payload`, offered as `mime`, from `window`'s surface.
    ///
    /// The caller must have pressed a pointer button over that surface and must
    /// still be holding it: that press is the implicit grab, and a compositor
    /// refuses a drag without one.
    ///
    /// # Errors
    ///
    /// [`ShellError::Backend`] if no button press has been seen or the source
    /// could not be created.
    pub fn start(
        &mut self,
        shell: &dyn Shell,
        window: WindowId,
        mime: &core::ffi::CStr,
        payload: &[u8],
    ) -> Result<(), ShellError> {
        let SurfaceTarget::Wayland { surface, .. } = shell.surface_target(window)? else {
            return Err(ShellError::Backend("not a Wayland window".to_string()));
        };
        let serial = self.state.press_serial.get();
        if serial == 0 {
            return Err(ShellError::Backend(
                "no pointer button has been pressed, so there is no implicit grab to drag from"
                    .to_string(),
            ));
        }
        self.state.payload.replace(payload.to_vec());
        let origin: *mut WlProxy = surface.as_ptr().cast();
        // SAFETY: the manager, device and surface are all live on this
        // connection. `set_actions` must precede `start_drag`, and a null icon
        // is the protocol's own "no drag image" — the compositor draws nothing,
        // which is all a headless test needs.
        unsafe {
            self.state.source = wl_data_device_manager::create_data_source(self.state.manager);
            if self.state.source.is_null() {
                return Err(ShellError::Backend("create_data_source failed".to_string()));
            }
            let data = ptr::from_mut(self.state.as_mut()).cast::<c_void>();
            (self.lib.proxy_add_dispatcher)(
                self.state.source,
                drag_dispatch,
                data,
                ptr::null_mut(),
            );
            wl_data_source::offer(self.state.source, mime);
            if ffi::proxy_version(self.state.source) >= 3 {
                // Without an action the destination's `wl_data_offer.finish`
                // would be a protocol error, so the harness has to negotiate
                // one for the backend's accept path to be exercised at all.
                wl_data_source::set_actions(self.state.source, 1 /* copy */);
            }
            wl_data_device::start_drag(
                self.state.device,
                self.state.source,
                origin,
                ptr::null_mut(),
                serial,
            );
            (self.lib.display_flush)(self.display);
        }
        Ok(())
    }
}

impl Drop for DragSource {
    fn drop(&mut self) {
        // SAFETY: every proxy was created on this connection and each
        // destructor belongs to its own interface.
        unsafe {
            if !self.state.source.is_null() {
                wl_data_source::destroy(self.state.source);
            }
            if !self.state.device.is_null() {
                // `release` arrived in version 2 of the interface; sending a
                // request the bound version does not have is a protocol error
                // that disconnects the whole client, test and all.
                if ffi::proxy_version(self.state.device) >= 2 {
                    wl_data_device::release(self.state.device);
                } else {
                    (self.lib.proxy_destroy)(self.state.device);
                }
            }
            if !self.state.pointer.is_null() {
                // The seat above is bound at version 1, which has no
                // `wl_pointer.release` at all — so the proxy is dropped
                // client-side and the server-side object goes with the seat.
                (self.lib.proxy_destroy)(self.state.pointer);
            }
            if !self.state.manager.is_null() {
                (self.lib.proxy_destroy)(self.state.manager);
            }
            if !self.state.seat.is_null() {
                (self.lib.proxy_destroy)(self.state.seat);
            }
            (self.lib.proxy_destroy)(self.state.registry);
            (self.lib.display_flush)(self.display);
        }
    }
}

/// An `f64` as the signed 24.8 fixed point the wire carries.
#[expect(
    clippy::cast_possible_truncation,
    reason = "test-only input amounts are single-digit pixels"
)]
fn to_fixed(value: f64) -> i32 {
    (value * 256.0).round() as i32
}
