//! An input method for the end-to-end suite, so `text-input-v3` can be
//! tested through a real compositor's relay.
//!
//! Bound on the shell's own connection, as [`VirtualInput`](super::VirtualInput)
//! is and for its reason: the compositor relays a text input to whichever
//! client holds the seat's input method, and nothing stops that being the same
//! client. Its events are dispatched by the shell's own pump, so a test pumps
//! the shell and then asks [`InputMethod::is_active`].

use core::ffi::{CStr, c_int, c_void};
use core::ptr;

use crcbl_core::SurfaceTarget;

use super::super::ffi::{self, WlArgument, WlDisplay, WlMessage, WlProxy};
use super::super::protocol::input_method::{zwp_input_method_manager_v2, zwp_input_method_v2};
use super::super::protocol::wayland::{wl_display, wl_registry, wl_seat};
use super::ignore;
use crate::{Shell, ShellError, WindowId};

/// What the registry and the input method have said.
struct State {
    registry: *mut WlProxy,
    seat: *mut WlProxy,
    manager: *mut WlProxy,
    method: *mut WlProxy,
    /// `activate` or `deactivate`, until the `done` that applies it.
    pending_active: bool,
    /// Whether a text input is enabled, as of the last `done`.
    active: bool,
    /// `content_type`'s hint and purpose, until the `done` that applies it.
    pending_content: (u32, u32),
    /// The enabled text input's hint and purpose, as of the last `done`.
    content: (u32, u32),
    /// How many `done` events have arrived: the serial `commit` must quote.
    done: u32,
    /// The compositor already has an input method.
    unavailable: bool,
}

/// Binds `wl_seat` and `zwp_input_method_manager_v2`, ignoring everything
/// else.
unsafe extern "C" fn bind(
    user_data: *const c_void,
    _target: *mut c_void,
    opcode: u32,
    _message: *const WlMessage,
    args: *mut WlArgument,
) -> c_int {
    // SAFETY: `user_data` is the boxed `State` of an `InputMethod`, which
    // outlives the round trip that can call this.
    let state = unsafe { &mut *user_data.cast::<State>().cast_mut() };
    // SAFETY: attached only to a `wl_registry`, so `args` matches its events.
    let event = unsafe { wl_registry::decode_event(opcode, &*args.cast_const()) };
    let Some(wl_registry::Event::Global {
        name, interface, ..
    }) = event
    else {
        return 0;
    };
    // SAFETY: the registry is live; binding from inside a dispatch only queues
    // the request.
    unsafe {
        if interface == wl_seat::NAME && state.seat.is_null() {
            state.seat = wl_registry::bind(state.registry, name, &wl_seat::INTERFACE, 1);
        } else if interface == zwp_input_method_manager_v2::NAME && state.manager.is_null() {
            state.manager = wl_registry::bind(
                state.registry,
                name,
                &zwp_input_method_manager_v2::INTERFACE,
                1,
            );
        }
    }
    0
}

/// Follows the input method's double-buffered state.
unsafe extern "C" fn follow(
    user_data: *const c_void,
    _target: *mut c_void,
    opcode: u32,
    _message: *const WlMessage,
    args: *mut WlArgument,
) -> c_int {
    // SAFETY: as in `bind`; the box lives as long as the proxy.
    let state = unsafe { &mut *user_data.cast::<State>().cast_mut() };
    // SAFETY: attached only to a `zwp_input_method_v2`.
    match unsafe { zwp_input_method_v2::decode_event(opcode, &*args.cast_const()) } {
        Some(zwp_input_method_v2::Event::Activate) => state.pending_active = true,
        Some(zwp_input_method_v2::Event::Deactivate) => state.pending_active = false,
        Some(zwp_input_method_v2::Event::ContentType { hint, purpose }) => {
            state.pending_content = (hint, purpose);
        }
        Some(zwp_input_method_v2::Event::Done) => {
            state.done = state.done.wrapping_add(1);
            state.active = state.pending_active;
            state.content = state.pending_content;
        }
        Some(zwp_input_method_v2::Event::Unavailable) => state.unavailable = true,
        _ => {}
    }
    0
}

/// The seat's input method, played by the test.
pub struct InputMethod {
    lib: &'static ffi::Lib,
    display: *mut WlDisplay,
    state: Box<State>,
}

impl core::fmt::Debug for InputMethod {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("InputMethod")
            .field("active", &self.state.active)
            .field("done", &self.state.done)
            .finish()
    }
}

impl InputMethod {
    /// Becomes the input method of `shell`'s compositor's seat.
    ///
    /// # Errors
    ///
    /// [`ShellError::Backend`] if `window` is not a Wayland window, or the
    /// compositor has no `wl_seat`, no `zwp_input_method_manager_v2`, or
    /// already has an input method.
    pub fn attach(shell: &dyn Shell, window: WindowId) -> Result<Self, ShellError> {
        let SurfaceTarget::Wayland { display, .. } = shell.surface_target(window)? else {
            return Err(ShellError::Backend(
                "not a Wayland window; this helper is only for the Wayland backend".to_string(),
            ));
        };
        let lib = ffi::load().map_err(|detail| ShellError::Backend(detail.to_string()))?;
        let display: *mut WlDisplay = display.as_ptr().cast();
        let mut state = Box::new(State {
            registry: ptr::null_mut(),
            seat: ptr::null_mut(),
            manager: ptr::null_mut(),
            method: ptr::null_mut(),
            pending_active: false,
            active: false,
            pending_content: (0, 0),
            content: (0, 0),
            done: 0,
            unavailable: false,
        });
        let user_data: *const c_void = ptr::from_mut(state.as_mut()).cast();
        // SAFETY: the display belongs to the shell and outlives this borrow;
        // `state` is boxed, so the pointer each dispatcher holds stays valid
        // for this value's life, which outlasts every proxy it is attached to.
        unsafe {
            state.registry = wl_display::get_registry(ffi::display_as_proxy(display));
            if state.registry.is_null() {
                return Err(ShellError::Backend("wl_display.get_registry".to_string()));
            }
            (lib.proxy_add_dispatcher)(state.registry, bind, user_data, ptr::null_mut());
            if (lib.display_roundtrip)(display) < 0 {
                return Err(ShellError::Backend("roundtrip failed".to_string()));
            }
        }
        let mut method = Self {
            lib,
            display,
            state,
        };
        if method.state.seat.is_null() || method.state.manager.is_null() {
            return Err(ShellError::Backend(
                "the compositor has no wl_seat or zwp_input_method_manager_v2".to_string(),
            ));
        }
        // SAFETY: the seat and manager are live on this connection, and the
        // input method's dispatcher reads the same boxed state.
        unsafe {
            (lib.proxy_add_dispatcher)(method.state.seat, ignore, ptr::null(), ptr::null_mut());
            method.state.method = zwp_input_method_manager_v2::get_input_method(
                method.state.manager,
                method.state.seat,
            );
            if method.state.method.is_null() {
                return Err(ShellError::Backend(
                    "zwp_input_method_manager_v2.get_input_method".to_string(),
                ));
            }
            (lib.proxy_add_dispatcher)(method.state.method, follow, user_data, ptr::null_mut());
            (lib.display_roundtrip)(display);
        }
        if method.state.unavailable {
            return Err(ShellError::Backend(
                "the compositor already has an input method".to_string(),
            ));
        }
        method.flush();
        Ok(method)
    }

    /// Whether a text input is enabled, as of the last `done` the shell's
    /// pump dispatched.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.state.active
    }

    /// The enabled text input's `content_type`, hint and purpose, as of the
    /// last `done` the shell's pump dispatched.
    #[must_use]
    pub fn content_type(&self) -> (u32, u32) {
        self.state.content
    }

    /// Sends a pre-edit with its cursor at `cursor` bytes in.
    pub fn preedit(&self, text: &CStr, cursor: i32) {
        // SAFETY: the input method is live; `commit` quotes the count of
        // `done` events, which is the serial the protocol asks for.
        unsafe {
            zwp_input_method_v2::set_preedit_string(self.state.method, text, cursor, cursor);
            zwp_input_method_v2::commit(self.state.method, self.state.done);
        }
        self.flush();
    }

    /// Commits `text`, which also ends any pre-edit, since none is sent with
    /// it.
    pub fn commit(&self, text: &CStr) {
        // SAFETY: as in `preedit`.
        unsafe {
            zwp_input_method_v2::commit_string(self.state.method, text);
            zwp_input_method_v2::commit(self.state.method, self.state.done);
        }
        self.flush();
    }

    fn flush(&self) {
        // SAFETY: the display is live for as long as the shell that owns it.
        unsafe { (self.lib.display_flush)(self.display) };
    }
}

impl Drop for InputMethod {
    fn drop(&mut self) {
        // SAFETY: every proxy was made on this connection and each destructor
        // is its own interface's; the seat has none, so it is destroyed
        // client-side.
        unsafe {
            if !self.state.method.is_null() {
                zwp_input_method_v2::destroy(self.state.method);
            }
            if !self.state.manager.is_null() {
                zwp_input_method_manager_v2::destroy(self.state.manager);
            }
            if !self.state.seat.is_null() {
                (self.lib.proxy_destroy)(self.state.seat);
            }
            (self.lib.proxy_destroy)(self.state.registry);
            (self.lib.display_flush)(self.display);
        }
    }
}
