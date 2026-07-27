//! Hand-written FFI to libwayland-client, and the runtime facade the generated
//! protocol code marshals through.
//!
//! # Decision: `dlopen`, not `#[link]`
//!
//! `docs/plan/15-windowing.md` accepts "thin bindings to APIs the OS or driver
//! requires *by ABI*", which is what this file is — fourteen libwayland entry
//! points plus five libc ones, declared by hand and audited by use. What it does **not** settle is how the library is
//! reached, and the two options are not equivalent:
//!
//! * `#[link(name = "wayland-client")]` puts `libwayland-client.so.0` in the
//!   binary's `DT_NEEDED`. A machine without it — a server running `crcbl sim`,
//!   an X11-only desktop, a minimal CI container — then fails at `execve`, in
//!   `ld.so`, before `main`. [`open`](crate::open)'s registry, which exists
//!   precisely to try Wayland and fall back to X11 or headless, never runs. A
//!   fallback that a dynamic loader can pre-empt is not a fallback.
//! * `dlopen` at [`Lib::load`] time makes a missing library an ordinary
//!   [`ShellError::Connect`], the registry moves on to the next backend, and
//!   the log line says which library was missing.
//!
//! Packaging pushes the same way: with `DT_NEEDED` the engine package must
//! *depend* on libwayland, so installing the game on a headless box drags in a
//! display library; with `dlopen` it is a recommendation. This is the same
//! shape the Vulkan loader, SDL and Mesa all use for the same reason.
//!
//! The cost is real and worth stating: symbols are resolved by hand, so a
//! typo'd name is a runtime error rather than a link error. The mitigation is
//! that [`Lib::load`] resolves **every** symbol eagerly and fails as a whole —
//! there is no lazily-missing function that surfaces three hours into a
//! session.
//!
//! ## Sharing the library with the Vulkan WSI
//!
//! `crcbl-vk` will hand the `wl_display*` and `wl_surface*` from
//! [`SurfaceTarget::Wayland`](crcbl_core::SurfaceTarget) to
//! `vkCreateWaylandSurfaceKHR`, and the driver calls libwayland functions on
//! them. That is safe across the `dlopen` boundary: the driver's own
//! `DT_NEEDED` (or `dlopen`) on `libwayland-client.so.0` resolves to the
//! object already mapped in this process — one copy of the library, one set of
//! globals, one connection state. `dlopen` of an already-loaded soname does not
//! produce a second instance.
//!
//! # Decision: `wl_proxy_marshal_array_flags`, not the variadic form
//!
//! libwayland exposes both. The variadic `wl_proxy_marshal_flags` is what the C
//! headers' inline wrappers use because C makes it convenient; from Rust it
//! means constructing a C variadic call for every request, where the argument
//! promotion rules are the caller's problem and a mistake is silent. The array
//! form takes a `union wl_argument *` — exactly the shape generated code wants
//! to build anyway — and has no variadic ABI in it at all. It is the entry
//! point libwayland's own non-inline path uses.

use core::ffi::{CStr, c_char, c_int, c_long, c_ulong, c_void};
use core::ptr;
use std::sync::OnceLock;

// ---------------------------------------------------------------------------
// Opaque libwayland types
// ---------------------------------------------------------------------------

/// `struct wl_proxy` — an object on the connection.
///
/// Opaque: libwayland owns the layout and we never look inside. Declared as an
/// empty `enum`-shaped struct so a `*mut WlProxy` cannot be accidentally
/// dereferenced.
#[repr(C)]
pub struct WlProxy {
    _opaque: [u8; 0],
}

/// `struct wl_display` — the connection.
///
/// A `wl_display*` is also a `wl_proxy*` (libwayland puts the proxy first in
/// the struct, and its own headers cast between them); [`display_as_proxy`]
/// does that cast in one audited place.
#[repr(C)]
pub struct WlDisplay {
    _opaque: [u8; 0],
}

/// `struct wl_array` — a length-prefixed blob, as carried by array arguments.
#[repr(C)]
pub struct WlArray {
    /// Bytes in use.
    pub size: usize,
    /// Bytes allocated.
    pub alloc: usize,
    /// The bytes.
    pub data: *mut c_void,
}

/// `struct wl_message` — one request or event descriptor.
///
/// Generated code builds these as `static`s. libwayland reads `signature` to
/// encode and decode the wire, and `types` to know what interface each object
/// argument refers to.
#[repr(C)]
pub struct WlMessage {
    /// Message name, NUL-terminated.
    pub name: *const c_char,
    /// Argument signature; see `crcbl-wl-scanner`.
    pub signature: *const c_char,
    /// One `wl_interface*` (or null) per wire argument slot.
    pub types: *const *const WlInterface,
}

// SAFETY: a `WlMessage` is only ever constructed as an immutable `static` whose
// pointers all name other immutable `static`s in the same binary. Nothing
// mutates one, so sharing it across threads cannot race. The generated code is
// the only constructor.
unsafe impl Sync for WlMessage {}

impl WlMessage {
    /// Builds a message descriptor. Called only from generated code.
    #[must_use]
    pub const fn new(
        name: &'static CStr,
        signature: &'static CStr,
        types: *const *const WlInterface,
    ) -> Self {
        Self {
            name: name.as_ptr(),
            signature: signature.as_ptr(),
            types,
        }
    }
}

/// `struct wl_interface` — the descriptor libwayland *and the graphics driver*
/// read for every object.
///
/// This is why the type tables have to be exactly right: `vkCreateWaylandSurfaceKHR`
/// takes our `wl_surface*`, and the driver marshals `wl_surface.attach` and
/// friends on it using the interface pointer *stored in the proxy* — which is
/// the one below, not the driver's own copy.
#[repr(C)]
pub struct WlInterface {
    /// Wire name, NUL-terminated.
    pub name: *const c_char,
    /// Interface version this descriptor covers.
    pub version: c_int,
    /// Number of requests.
    pub method_count: c_int,
    /// Request descriptors, in opcode order.
    pub methods: *const WlMessage,
    /// Number of events.
    pub event_count: c_int,
    /// Event descriptors, in opcode order.
    pub events: *const WlMessage,
}

// SAFETY: as for `WlMessage` — immutable statics pointing at immutable statics.
unsafe impl Sync for WlInterface {}

impl WlInterface {
    /// Builds an interface descriptor. Called only from generated code.
    #[must_use]
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        reason = "counts come from array lengths in generated code; the largest \
                  interface in any Wayland protocol has fewer than 100 messages"
    )]
    pub const fn new(
        name: &'static CStr,
        version: u32,
        methods: &'static [WlMessage],
        events: &'static [WlMessage],
    ) -> Self {
        Self {
            name: name.as_ptr(),
            version: version as c_int,
            method_count: methods.len() as c_int,
            methods: methods.as_ptr(),
            event_count: events.len() as c_int,
            events: events.as_ptr(),
        }
    }
}

/// One entry of a protocol's type table.
///
/// A newtype rather than a bare `*const WlInterface` so the table can be a
/// `static` — a raw pointer is not `Sync`, and this is the one place where
/// asserting otherwise is justified.
#[repr(transparent)]
pub struct WlInterfacePtr(pub *const WlInterface);

// SAFETY: every value is either null or the address of an immutable `static`
// `WlInterface` in this binary. Nothing mutates the table or its targets.
unsafe impl Sync for WlInterfacePtr {}

impl WlInterfacePtr {
    /// The `NULL` entry, for an argument that names no object.
    pub const NULL: Self = Self(ptr::null());

    /// An entry naming `interface`.
    #[must_use]
    pub const fn new(interface: &'static WlInterface) -> Self {
        Self(ptr::from_ref(interface))
    }
}

/// `union wl_argument` — one wire argument, in libwayland's in-memory form.
#[repr(C)]
pub union WlArgument {
    /// `int`.
    pub i: i32,
    /// `uint`.
    pub u: u32,
    /// `fixed`, 24.8 signed.
    pub f: i32,
    /// `string`.
    pub s: *const c_char,
    /// `object`, and `new_id` once libwayland has created the proxy.
    pub o: *mut WlProxy,
    /// `new_id` as an id on the wire.
    pub n: u32,
    /// `array`.
    pub a: *const WlArray,
    /// `fd`.
    pub h: i32,
}

/// libwayland's generic event dispatcher.
///
/// `int (*)(const void *user_data, void *target, uint32_t opcode,
///          const struct wl_message *msg, union wl_argument *args)`
pub type DispatcherFn = unsafe extern "C" fn(
    *const c_void,
    *mut c_void,
    u32,
    *const WlMessage,
    *mut WlArgument,
) -> c_int;

/// `WL_MARSHAL_FLAG_DESTROY` — this request destroys the proxy it is sent on.
pub const MARSHAL_FLAG_DESTROY: u32 = 1;

// ---------------------------------------------------------------------------
// The library
// ---------------------------------------------------------------------------

// `dlopen`/`dlsym` are declared here rather than taken from a crate because
// they are three functions with a stable ABI, and `libc` would be a dependency
// in the engine's graph for exactly that. The symbols come from libc, which
// `std` already links on every Unix target.
unsafe extern "C" {
    fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlerror() -> *const c_char;
    fn poll(fds: *mut PollFd, nfds: c_ulong, timeout: c_int) -> c_int;
    fn clock_gettime(clock_id: c_int, tp: *mut Timespec) -> c_int;
}

/// `struct pollfd`.
#[repr(C)]
struct PollFd {
    fd: c_int,
    events: i16,
    revents: i16,
}

/// `struct timespec`.
#[repr(C)]
struct Timespec {
    tv_sec: c_long,
    tv_nsec: c_long,
}

/// `RTLD_NOW` — resolve everything up front, so a partially usable library is
/// not a thing that can happen.
const RTLD_NOW: c_int = 2;
/// `POLLIN`.
const POLLIN: i16 = 0x001;
/// `CLOCK_MONOTONIC` — the clock Wayland stamps input events with.
const CLOCK_MONOTONIC: c_int = 1;

/// The C prototypes this backend asserts about libwayland-client.
///
/// Written out as aliases rather than inline in [`Lib`] so that the *same*
/// type appears in the struct field and in the `transmute` that fills it —
/// `dlsym` returns `void*`, and the signature is entirely our assertion. Every
/// one is copied from `wayland-client-core.h` and is ABI-stable across all of
/// libwayland 1.x.
mod prototype {
    use super::{DispatcherFn, WlArgument, WlDisplay, WlInterface, WlProxy, c_char, c_int, c_void};

    /// `struct wl_display *wl_display_connect(const char *name)`
    pub type DisplayConnect = unsafe extern "C" fn(*const c_char) -> *mut WlDisplay;
    /// `void wl_display_disconnect(struct wl_display *)`
    pub type DisplayVoid = unsafe extern "C" fn(*mut WlDisplay);
    /// `int wl_display_get_fd(struct wl_display *)`, and every other
    /// `int (*)(struct wl_display *)` in the list.
    pub type DisplayInt = unsafe extern "C" fn(*mut WlDisplay) -> c_int;
    /// `struct wl_proxy *wl_proxy_marshal_array_flags(struct wl_proxy *,
    /// uint32_t opcode, const struct wl_interface *, uint32_t version,
    /// uint32_t flags, union wl_argument *args)`
    pub type ProxyMarshalArrayFlags = unsafe extern "C" fn(
        *mut WlProxy,
        u32,
        *const WlInterface,
        u32,
        u32,
        *mut WlArgument,
    ) -> *mut WlProxy;
    /// `int wl_proxy_add_dispatcher(struct wl_proxy *, wl_dispatcher_func_t,
    /// const void *dispatcher_data, void *data)`
    pub type ProxyAddDispatcher =
        unsafe extern "C" fn(*mut WlProxy, DispatcherFn, *const c_void, *mut c_void) -> c_int;
    /// `void wl_proxy_destroy(struct wl_proxy *)`
    pub type ProxyVoid = unsafe extern "C" fn(*mut WlProxy);
    /// `uint32_t wl_proxy_get_version(struct wl_proxy *)`
    pub type ProxyU32 = unsafe extern "C" fn(*mut WlProxy) -> u32;
}

/// Every libwayland-client entry point this backend uses.
///
/// Fourteen functions. Each one is used; a function that stops being used is
/// deleted from this struct in the same commit, which is what "audited by use"
/// means in `docs/plan/15-windowing.md`.
pub struct Lib {
    pub display_connect: prototype::DisplayConnect,
    pub display_disconnect: prototype::DisplayVoid,
    pub display_get_fd: prototype::DisplayInt,
    pub display_roundtrip: prototype::DisplayInt,
    pub display_dispatch_pending: prototype::DisplayInt,
    pub display_flush: prototype::DisplayInt,
    pub display_prepare_read: prototype::DisplayInt,
    pub display_read_events: prototype::DisplayInt,
    pub display_cancel_read: prototype::DisplayVoid,
    pub display_get_error: prototype::DisplayInt,
    pub proxy_marshal_array_flags: prototype::ProxyMarshalArrayFlags,
    pub proxy_add_dispatcher: prototype::ProxyAddDispatcher,
    pub proxy_destroy: prototype::ProxyVoid,
    pub proxy_get_version: prototype::ProxyU32,
}

impl core::fmt::Debug for Lib {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Lib(libwayland-client)")
    }
}

/// The sonames tried, in order.
///
/// The versioned one first: `libwayland-client.so` is a symlink shipped only by
/// the `-dev`/`-devel` package, so a machine with a Wayland session but no
/// development headers has only the first.
const SONAMES: [&CStr; 2] = [c"libwayland-client.so.0", c"libwayland-client.so"];

static LIB: OnceLock<Result<Lib, String>> = OnceLock::new();

/// Loads libwayland-client, once per process.
///
/// # Errors
///
/// The `dlerror` text if no soname could be opened, or the name of the first
/// symbol that could not be resolved. Both are reported as a string because
/// they are for a human reading a log line, not for a caller to branch on.
pub fn load() -> Result<&'static Lib, &'static str> {
    match LIB.get_or_init(load_uncached) {
        Ok(lib) => Ok(lib),
        Err(message) => Err(message.as_str()),
    }
}

/// The loaded library.
///
/// # Panics
///
/// If the library was never loaded. Unreachable in practice: nothing calls this
/// except generated protocol code, which only runs on a connection that
/// [`load`] already succeeded for.
#[inline]
#[must_use]
pub fn lib() -> &'static Lib {
    load().expect("libwayland-client is loaded before any protocol code runs")
}

fn load_uncached() -> Result<Lib, String> {
    let mut handle = ptr::null_mut();
    for soname in SONAMES {
        // SAFETY: `soname` is a NUL-terminated literal, and `RTLD_NOW` is a
        // valid flag. `dlopen` either returns a handle or null; it does not
        // take ownership of the string.
        handle = unsafe { dlopen(soname.as_ptr(), RTLD_NOW) };
        if !handle.is_null() {
            break;
        }
    }
    if handle.is_null() {
        // SAFETY: `dlerror` returns a pointer to a static buffer owned by the
        // loader, valid until the next call on this thread. It is copied out
        // immediately.
        let detail = unsafe {
            let message = dlerror();
            if message.is_null() {
                "not found".to_string()
            } else {
                CStr::from_ptr(message).to_string_lossy().into_owned()
            }
        };
        return Err(format!("cannot load libwayland-client.so.0: {detail}"));
    }

    /// Resolves one symbol, or reports which one was missing.
    ///
    /// The `transmute` is the unavoidable core of `dlopen`-based loading:
    /// `dlsym` returns `void*` and the caller asserts the signature. The
    /// asserted type is named at every use site and defined, with its C
    /// prototype, in [`prototype`].
    macro_rules! symbol {
        ($name:literal, $ty:ty) => {{
            // SAFETY: `handle` is a live `dlopen` handle and the name is a
            // NUL-terminated literal.
            let raw = unsafe { dlsym(handle, concat!($name, "\0").as_ptr().cast()) };
            if raw.is_null() {
                return Err(format!("libwayland-client.so.0 has no symbol {}", $name));
            }
            // SAFETY: `$ty` is the prototype `wayland-client-core.h` declares
            // for this symbol, and libwayland's ABI is stable across 1.x.
            unsafe { core::mem::transmute::<*mut c_void, $ty>(raw) }
        }};
    }

    Ok(Lib {
        display_connect: symbol!("wl_display_connect", prototype::DisplayConnect),
        display_disconnect: symbol!("wl_display_disconnect", prototype::DisplayVoid),
        display_get_fd: symbol!("wl_display_get_fd", prototype::DisplayInt),
        display_roundtrip: symbol!("wl_display_roundtrip", prototype::DisplayInt),
        display_dispatch_pending: symbol!("wl_display_dispatch_pending", prototype::DisplayInt),
        display_flush: symbol!("wl_display_flush", prototype::DisplayInt),
        display_prepare_read: symbol!("wl_display_prepare_read", prototype::DisplayInt),
        display_read_events: symbol!("wl_display_read_events", prototype::DisplayInt),
        display_cancel_read: symbol!("wl_display_cancel_read", prototype::DisplayVoid),
        display_get_error: symbol!("wl_display_get_error", prototype::DisplayInt),
        proxy_marshal_array_flags: symbol!(
            "wl_proxy_marshal_array_flags",
            prototype::ProxyMarshalArrayFlags
        ),
        proxy_add_dispatcher: symbol!("wl_proxy_add_dispatcher", prototype::ProxyAddDispatcher),
        proxy_destroy: symbol!("wl_proxy_destroy", prototype::ProxyVoid),
        proxy_get_version: symbol!("wl_proxy_get_version", prototype::ProxyU32),
    })
}

// ---------------------------------------------------------------------------
// The facade generated code marshals through
// ---------------------------------------------------------------------------

/// Sends one request.
///
/// The single point every generated request wrapper funnels through, so the
/// `unsafe` surface of the protocol layer is one function wide.
///
/// # Safety
///
/// `proxy` must be live on an open connection; `args` must match the signature
/// of `opcode` on that proxy's interface and stay valid for the call;
/// `interface` must be null, or the interface of the object this request
/// creates.
#[inline]
pub unsafe fn marshal(
    proxy: *mut WlProxy,
    opcode: u32,
    interface: *const WlInterface,
    version: u32,
    flags: u32,
    args: *mut WlArgument,
) -> *mut WlProxy {
    // SAFETY: forwarded to libwayland under the caller's obligations, which are
    // exactly libwayland's own for this entry point.
    unsafe { (lib().proxy_marshal_array_flags)(proxy, opcode, interface, version, flags, args) }
}

/// The version a proxy was bound at.
///
/// # Safety
///
/// `proxy` must be live on an open connection.
#[inline]
#[must_use]
pub unsafe fn proxy_version(proxy: *mut WlProxy) -> u32 {
    // SAFETY: the caller guarantees `proxy` is live.
    unsafe { (lib().proxy_get_version)(proxy) }
}

/// A non-nullable string argument.
///
/// A null pointer yields the empty string rather than panicking: a compositor
/// that sends one is misbehaving, and taking down the game for it helps nobody.
///
/// # Safety
///
/// `ptr` must be null or a NUL-terminated string valid for the returned
/// lifetime — which for an event argument means "until the dispatcher returns".
#[inline]
#[must_use]
pub unsafe fn cstr_arg<'a>(ptr: *const c_char) -> &'a CStr {
    if ptr.is_null() {
        return c"";
    }
    // SAFETY: the caller guarantees a NUL-terminated string valid for `'a`.
    unsafe { CStr::from_ptr(ptr) }
}

/// A nullable string argument.
///
/// # Safety
///
/// As [`cstr_arg`].
#[inline]
#[must_use]
pub unsafe fn cstr_arg_opt<'a>(ptr: *const c_char) -> Option<&'a CStr> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: the caller guarantees a NUL-terminated string valid for `'a`.
    Some(unsafe { CStr::from_ptr(ptr) })
}

/// An array argument, as bytes.
///
/// # Safety
///
/// `ptr` must be null or a `wl_array` whose `data`/`size` describe an
/// initialised region valid for the returned lifetime.
#[inline]
#[must_use]
pub unsafe fn array_arg<'a>(ptr: *const WlArray) -> &'a [u8] {
    if ptr.is_null() {
        return &[];
    }
    // SAFETY: the caller guarantees `ptr` describes an initialised region of
    // `size` bytes that outlives `'a`. A zero-size array may carry a null
    // `data`, which `from_raw_parts` forbids, so that case returns empty.
    unsafe {
        let array = &*ptr;
        if array.data.is_null() || array.size == 0 {
            &[]
        } else {
            core::slice::from_raw_parts(array.data.cast::<u8>(), array.size)
        }
    }
}

/// Reinterprets a `wl_display*` as the `wl_proxy*` it also is.
///
/// libwayland's own headers do this cast in every `wl_display_*` inline
/// wrapper; `struct wl_display` starts with its `struct wl_proxy`. Confining it
/// to one function keeps the assumption findable.
#[inline]
#[must_use]
pub const fn display_as_proxy(display: *mut WlDisplay) -> *mut WlProxy {
    display.cast()
}

/// Whether `fd` is readable right now.
///
/// A zero timeout makes this a poll in the strict sense — it never blocks,
/// which is what [`Shell::pump`](crate::Shell::pump)'s finite-drain contract
/// requires. `timeout_ms` of `-1` blocks indefinitely, which is what
/// [`Shell::wait_events`](crate::Shell::wait_events) wants.
pub fn poll_readable(fd: c_int, timeout_ms: c_int) -> bool {
    let mut fds = PollFd {
        fd,
        events: POLLIN,
        revents: 0,
    };
    // SAFETY: `fds` is one initialised `pollfd` and the count matches.
    let result = unsafe { poll(&raw mut fds, 1, timeout_ms) };
    result > 0 && fds.revents & POLLIN != 0
}

/// `CLOCK_MONOTONIC` in nanoseconds — the clock Wayland stamps input events
/// with.
///
/// Returns zero if the call fails, which cannot happen for `CLOCK_MONOTONIC` on
/// any Linux this engine supports; propagating an error for it would put a
/// `Result` on every timestamp for no reachable case.
#[must_use]
pub fn monotonic_nanos() -> u64 {
    let mut ts = Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `ts` is an initialised `timespec` and `CLOCK_MONOTONIC` is valid.
    if unsafe { clock_gettime(CLOCK_MONOTONIC, &raw mut ts) } != 0 {
        return 0;
    }
    let seconds = u64::try_from(ts.tv_sec).unwrap_or(0);
    let nanos = u64::try_from(ts.tv_nsec).unwrap_or(0);
    seconds * 1_000_000_000 + nanos
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_descriptor_types_match_the_c_layout() {
        // Getting these wrong would hand libwayland a struct it reads past the
        // end of. The C definitions are three and six pointer-sized words.
        assert_eq!(
            size_of::<WlMessage>(),
            3 * size_of::<*const c_void>(),
            "struct wl_message is name + signature + types"
        );
        assert_eq!(
            size_of::<WlInterface>(),
            5 * size_of::<*const c_void>(),
            "struct wl_interface is name, version+method_count, methods, \
             event_count+padding, events"
        );
        assert_eq!(
            size_of::<WlArgument>(),
            size_of::<*const c_void>(),
            "union wl_argument is one pointer wide"
        );
        assert_eq!(align_of::<WlArgument>(), align_of::<*const c_void>());
        assert_eq!(size_of::<WlInterfacePtr>(), size_of::<*const c_void>());
    }

    #[test]
    fn the_monotonic_clock_advances_and_is_not_the_epoch() {
        let first = monotonic_nanos();
        assert!(first > 0, "CLOCK_MONOTONIC is available on Linux");
        let second = monotonic_nanos();
        assert!(second >= first, "CLOCK_MONOTONIC never goes backwards");
    }

    #[test]
    fn a_null_array_argument_is_an_empty_slice_rather_than_a_crash() {
        // SAFETY: a null pointer is the documented "no array" case.
        assert_eq!(unsafe { array_arg(ptr::null()) }, &[] as &[u8]);
        let empty = WlArray {
            size: 0,
            alloc: 0,
            data: ptr::null_mut(),
        };
        // SAFETY: `empty` is an initialised `wl_array` describing no bytes.
        assert_eq!(unsafe { array_arg(&raw const empty) }, &[] as &[u8]);

        let bytes = [1u8, 2, 3, 4];
        let array = WlArray {
            size: bytes.len(),
            alloc: bytes.len(),
            data: bytes.as_ptr().cast_mut().cast(),
        };
        // SAFETY: `array` describes `bytes`, which outlives the borrow.
        assert_eq!(unsafe { array_arg(&raw const array) }, &bytes);
    }

    #[test]
    fn null_strings_degrade_to_empty_rather_than_panicking() {
        // SAFETY: null is the documented degenerate case for both helpers.
        unsafe {
            assert_eq!(cstr_arg(ptr::null()), c"");
            assert_eq!(cstr_arg_opt(ptr::null()), None);
            assert_eq!(cstr_arg(c"DP-2".as_ptr()), c"DP-2");
        }
    }

    #[test]
    fn polling_a_closed_descriptor_does_not_block() {
        // -1 is never readable; the point is that a zero timeout returns.
        assert!(!poll_readable(-1, 0));
    }
}
