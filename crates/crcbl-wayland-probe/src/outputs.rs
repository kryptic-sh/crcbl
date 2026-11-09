//! The heads and modes `zwlr_output_manager_v1` reports.

use core::ffi::{c_int, c_void};
use core::ptr;

use crcbl_wayland::Display;
use crcbl_wayland::ffi::{self, Lib, WlArgument, WlMessage, WlProxy};

use crate::protocol::wayland::{wl_display, wl_registry};
use crate::protocol::wlr_output_management::{
    zwlr_output_head_v1, zwlr_output_manager_v1, zwlr_output_mode_v1,
};

/// The newest `zwlr_output_manager_v1` this probe speaks: the version of the
/// vendored XML.
const MANAGER_VERSION: u32 = 4;

/// Round trips allowed before the manager's first `done`. It follows the
/// heads it describes in the reply to the bind, so one or two suffice; the cap
/// turns a compositor that never sends it into an error rather than a hang.
const MAX_ROUNDTRIPS: usize = 8;

/// One mode a head can be driven at.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mode {
    /// Width in pixels.
    pub width: i32,
    /// Height in pixels.
    pub height: i32,
    /// Refresh rate in mHz; zero when the compositor did not say.
    pub refresh_mhz: i32,
    /// Whether the display marks this mode as its preferred one.
    pub preferred: bool,
}

/// One output the compositor knows of, enabled or not.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Head {
    /// The connector name, such as `HDMI-A-1`.
    pub name: String,
    /// The compositor's human-readable description.
    pub description: String,
    /// Whether the output is in use.
    pub enabled: bool,
    /// Every mode the head offers.
    pub modes: Vec<Mode>,
    /// The index into [`modes`](Self::modes) it is driven at, while enabled.
    pub current_mode: Option<usize>,
}

/// Connects to the compositor, binds `zwlr_output_manager_v1`, and returns
/// every head it describes.
///
/// # Errors
///
/// What a person reads: no compositor, no output manager, or a connection
/// that failed or never finished describing its heads.
pub fn read_heads() -> Result<Vec<Head>, String> {
    let display = Display::connect().map_err(|error| error.to_string())?;
    let lib = ffi::lib();
    let state = State::boxed(lib);
    let result = read(&display, state);
    // SAFETY: `state` came from `Box::into_raw` in `State::boxed`. Every proxy
    // that could dispatch into it was destroyed inside `read`, and nothing
    // dispatches outside `display`'s calls, so this is the last use.
    let state = unsafe { Box::from_raw(state) };
    result.map(|()| state.heads)
}

/// The part of [`read_heads`] that runs with the state allocated.
fn read(display: &Display, state: *mut State) -> Result<(), String> {
    let lost = |error: crcbl_wayland::Disconnected| format!("the connection failed: {error}");
    // SAFETY: the display is live, and `get_registry` takes it as the proxy
    // it is.
    let registry = unsafe { wl_display::get_registry(display.as_proxy()) };
    // SAFETY (each `&mut *state` below): `state` is live until `read_heads`
    // frees it, and the dispatcher — its only other user — runs only inside
    // `display`'s calls, never while one of these borrows is held.
    unsafe { &mut *state }.watch(registry, Kind::Registry);
    display.roundtrip().map_err(lost)?;

    let Some((name, version)) = unsafe { &*state }.manager else {
        // SAFETY: the registry is live and nothing else will destroy it.
        unsafe { (ffi::lib().proxy_destroy)(registry) };
        return Err("the compositor offers no zwlr_output_manager_v1".to_string());
    };
    // SAFETY: the registry is live, and `name` is the global it announced for
    // this interface, bound at a version both sides speak.
    let manager = unsafe {
        wl_registry::bind(
            registry,
            name,
            &zwlr_output_manager_v1::INTERFACE,
            version.min(MANAGER_VERSION),
        )
    };
    unsafe { &mut *state }.watch(manager, Kind::Manager);

    let mut result = Err("the output manager never finished describing its heads".to_string());
    for _ in 0..MAX_ROUNDTRIPS {
        if let Err(error) = display.roundtrip() {
            result = Err(lost(error));
            break;
        }
        if unsafe { &*state }.done {
            result = Ok(());
            break;
        }
    }
    unsafe { &mut *state }.destroy_all();
    result
}

/// What each proxy the dispatcher was attached to is.
#[derive(Clone, Copy, Debug)]
enum Kind {
    Registry,
    Manager,
    /// A head, by its index in [`State::heads`].
    Head(usize),
    /// A mode, by its head's index and its own in that head's modes.
    Mode(usize, usize),
}

/// Everything the dispatcher writes, reached only through the raw pointer
/// `State::boxed` returns.
struct State {
    lib: &'static Lib,
    /// This allocation's address, which every dispatcher is handed.
    root: *mut State,
    objects: Vec<(*mut WlProxy, Kind)>,
    /// `zwlr_output_manager_v1`'s registry name and version, once announced.
    manager: Option<(u32, u32)>,
    heads: Vec<Head>,
    /// Whether the manager has sent `done` for the heads so far.
    done: bool,
}

impl State {
    fn boxed(lib: &'static Lib) -> *mut Self {
        let raw = Box::into_raw(Box::new(Self {
            lib,
            root: ptr::null_mut(),
            objects: Vec::new(),
            manager: None,
            heads: Vec::new(),
            done: false,
        }));
        // SAFETY: `raw` was just returned by `Box::into_raw`; nothing else
        // refers to it yet.
        unsafe { (*raw).root = raw };
        raw
    }

    fn kind_of(&self, proxy: *mut WlProxy) -> Option<Kind> {
        self.objects
            .iter()
            .find(|(candidate, _)| *candidate == proxy)
            .map(|(_, kind)| *kind)
    }

    /// Routes `proxy`'s events to [`dispatch`].
    fn watch(&mut self, proxy: *mut WlProxy, kind: Kind) {
        if proxy.is_null() {
            return;
        }
        self.objects.push((proxy, kind));
        // SAFETY: `proxy` is live and has no dispatcher yet, `dispatch` has
        // libwayland's dispatcher signature, and `root` is the allocation's
        // own pointer, live until every proxy is destroyed.
        unsafe {
            (self.lib.proxy_add_dispatcher)(proxy, dispatch, self.root.cast(), ptr::null_mut())
        };
    }

    /// Destroys every proxy client-side. The compositor's objects go with the
    /// connection, which closes straight after.
    fn destroy_all(&mut self) {
        for (proxy, _) in self.objects.drain(..).rev() {
            // SAFETY: each proxy is live and destroyed exactly once, here.
            unsafe { (self.lib.proxy_destroy)(proxy) };
        }
    }
}

/// libwayland's dispatcher for every object the probe owns.
unsafe extern "C" fn dispatch(
    user_data: *const c_void,
    target: *mut c_void,
    opcode: u32,
    message: *const WlMessage,
    args: *mut WlArgument,
) -> c_int {
    // SAFETY: `user_data` is `State::root`, live while any proxy is, and no
    // other reference to the state is held during a dispatch.
    let state = unsafe { &mut *user_data.cast::<State>().cast_mut() };
    let Some(kind) = state.kind_of(target.cast()) else {
        return 0;
    };
    // SAFETY (each decoder): `kind` is the interface the proxy was created as,
    // so `args` is that interface's array for `opcode`, and every borrow is
    // copied out inside the closure, which is as far as the slice can reach.
    let decode = |args: &[WlArgument]| match kind {
        Kind::Registry => {
            if let Some(wl_registry::Event::Global {
                name,
                interface,
                version,
            }) = unsafe { wl_registry::decode_event(opcode, args) }
                && interface.to_bytes() == b"zwlr_output_manager_v1"
            {
                state.manager = Some((name, version));
            }
        }
        Kind::Manager => match unsafe { zwlr_output_manager_v1::decode_event(opcode, args) } {
            Some(zwlr_output_manager_v1::Event::Head { head }) => {
                state.heads.push(Head::default());
                state.watch(head, Kind::Head(state.heads.len() - 1));
            }
            Some(zwlr_output_manager_v1::Event::Done { .. }) => state.done = true,
            _ => {}
        },
        Kind::Head(index) => match unsafe { zwlr_output_head_v1::decode_event(opcode, args) } {
            Some(zwlr_output_head_v1::Event::Name { name }) => {
                state.heads[index].name = name.to_string_lossy().into_owned();
            }
            Some(zwlr_output_head_v1::Event::Description { description }) => {
                state.heads[index].description = description.to_string_lossy().into_owned();
            }
            Some(zwlr_output_head_v1::Event::Enabled { enabled }) => {
                state.heads[index].enabled = enabled != 0;
            }
            Some(zwlr_output_head_v1::Event::Mode { mode }) => {
                state.heads[index].modes.push(Mode::default());
                let slot = state.heads[index].modes.len() - 1;
                state.watch(mode, Kind::Mode(index, slot));
            }
            Some(zwlr_output_head_v1::Event::CurrentMode { mode }) => {
                if let Some(Kind::Mode(head, slot)) = state.kind_of(mode) {
                    state.heads[head].current_mode = Some(slot);
                }
            }
            _ => {}
        },
        Kind::Mode(head, slot) => {
            let mode = &mut state.heads[head].modes[slot];
            match unsafe { zwlr_output_mode_v1::decode_event(opcode, args) } {
                Some(zwlr_output_mode_v1::Event::Size { width, height }) => {
                    mode.width = width;
                    mode.height = height;
                }
                Some(zwlr_output_mode_v1::Event::Refresh { refresh }) => mode.refresh_mhz = refresh,
                Some(zwlr_output_mode_v1::Event::Preferred) => mode.preferred = true,
                _ => {}
            }
        }
    };
    // SAFETY: `message` and `args` are the descriptor and closure argument
    // array libwayland passed this dispatcher, unchanged while it runs.
    unsafe { ffi::with_dispatcher_args(message, args, decode) };
    0
}
