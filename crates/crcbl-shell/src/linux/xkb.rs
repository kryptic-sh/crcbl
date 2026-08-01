//! `dlopen`'d bindings to libxkbcommon, for keymap interpretation.
//!
//! Shared by both Linux backends, because the *keymap* is a Linux artefact and
//! not a protocol one — see [the module above](super). What differs between
//! them is only where the keymap comes from, and that is two constructors on
//! one type:
//!
//! | Backend | Source | Constructor |
//! | --- | --- | --- |
//! | Wayland | `wl_keyboard.keymap` sends a descriptor to `mmap` | [`Keymap::from_fd`] |
//! | X11 | the keymap lives on the server; libxkbcommon-x11 reads it over the connection | [`Keymap::from_x11_device`] |
//!
//! # Decision: libxkbcommon, not our own XKB parser
//!
//! `docs/plan/15-windowing.md` names "keymap handling (XKB parsing)" as a
//! specific cost risk, and this is the module that either pays it or does not.
//! It does not, and the reasoning is the same "bindings, not frameworks" test
//! the rest of the Linux backend is decided by:
//!
//! * **The keymap format is libxkbcommon's, not Wayland's.**
//!   `wl_keyboard.keymap_format.xkb_v1` is defined by pointing at "the format
//!   xkbcommon parses" — there is no independent specification of it anywhere.
//!   Writing our own parser would mean reverse-engineering a *reference
//!   implementation*, which is precisely the situation where the ABI, not the
//!   framework, is the dependency. It is the same argument
//!   [`ffi`](crate::wayland::ffi) makes about libwayland: the compositor hands us bytes
//!   in someone else's format and we do not get a vote.
//! * **It is genuinely large.** An `xkb_keymap` is four sections
//!   (`xkb_keycodes`, `xkb_types`, `xkb_compat`, `xkb_symbols`) of a real
//!   language with includes, virtual modifiers, key types with named levels,
//!   modifier maps, group handling and per-key repeat flags. The published
//!   grammar plus the level-lookup rules is several thousand lines of work
//!   whose failure mode is "the é key does nothing on a French keyboard" —
//!   discovered by a user, never by us, because we all type US-QWERTY.
//! * **It owns no policy.** There is no event loop in it, no window, no state
//!   machine we do not drive. We hand it a string and ask it questions; it
//!   never calls us. That is the line `15-windowing.md` draws, and this is on
//!   the accepted side of it — unlike winit or SDL, which would answer
//!   "should this window be fullscreen" for us.
//! * **We keep the part that matters.** Scancode → [`KeyCode`] — the mapping
//!   *gameplay bindings* are built on — is [our own static evdev
//!   table](super::keymap), not xkbcommon's. XKB is asked only for the things
//!   that are layout-dependent by definition: the keysym, the committed text,
//!   the modifier bits and whether a key auto-repeats. A layout-independent
//!   fact must never come from a layout database.
//!
//! # Degrading, not failing
//!
//! `dlopen` rather than `#[link]`, for exactly the reasons
//! [`ffi`](crate::wayland::ffi) states — a machine without libxkbcommon must produce a
//! working shell, not a process that dies in `ld.so`. When [`load`] fails, or
//! when the compositor sends `keymap_format.no_keymap`:
//!
//! | Fact | With libxkbcommon | Without |
//! | --- | --- | --- |
//! | [`KeyCode`] | our evdev table | our evdev table — **unchanged** |
//! | [`Scancode`](crcbl_core::input::Scancode) | the evdev code | the evdev code — **unchanged** |
//! | [`Keysym`](crcbl_core::input::Keysym) | the layout's symbol | [`Keysym::NONE`](crcbl_core::input::Keysym::NONE) |
//! | `TextCommit` | committed UTF-8 | never emitted |
//! | [`Modifiers`] | XKB's effective state | derived from held [`KeyCode`]s |
//! | key repeats at all | `xkb_keymap_key_repeats` | every key repeats |
//!
//! So WASD, Escape and every gameplay binding keep working with no
//! libxkbcommon at all; only text and rebind-menu labels are lost.
//! [`ShellCaps::TEXT_IME`](crate::ShellCaps) is cleared in that case, which is
//! how a consumer finds out without guessing.

use core::ffi::{CStr, c_char, c_int, c_void};
use core::ptr;
use std::os::fd::{AsRawFd, FromRawFd};
use std::sync::OnceLock;

use crcbl_core::KeyCode;
use crcbl_core::input::{Keysym, Modifiers};

// `mmap`/`munmap` are how the keymap gets out of the file descriptor the
// compositor sent. Declared here rather than pulled from a crate for the same
// reason `ffi` declares `dlopen`: two functions with a frozen ABI that `std`
// already links. Closing the descriptor is `std::fs::File`'s job — see
// [`Keymap::from_fd`], which adopts it so that every failure path below closes
// it exactly once.
unsafe extern "C" {
    fn mmap(
        addr: *mut c_void,
        length: usize,
        prot: c_int,
        flags: c_int,
        fd: c_int,
        offset: i64,
    ) -> *mut c_void;
    fn munmap(addr: *mut c_void, length: usize) -> c_int;
    fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

/// `PROT_READ`.
const PROT_READ: c_int = 1;
/// `MAP_PRIVATE` — required by `wl_keyboard` version 7 and up, and correct for
/// every earlier version too.
const MAP_PRIVATE: c_int = 2;
/// `MAP_FAILED`.
const MAP_FAILED: *mut c_void = usize::MAX as *mut c_void;
/// `RTLD_NOW`; see [`ffi`](crate::wayland::ffi).
const RTLD_NOW: c_int = 2;

/// `XKB_KEYMAP_FORMAT_TEXT_V1` — the only format there is.
const KEYMAP_FORMAT_TEXT_V1: c_int = 1;
/// `XKB_STATE_MODS_EFFECTIVE`.
const STATE_MODS_EFFECTIVE: c_int = 1 << 3;
/// `XKB_MOD_INVALID`.
const MOD_INVALID: u32 = u32::MAX;

/// Wayland reports evdev codes; XKB numbers the same keys eight higher.
///
/// The offset is X11's, inherited by XKB and by every keymap ever written for
/// it. Getting it wrong shifts the whole keyboard by eight keys, which produces
/// a plausible-looking keymap that is wrong everywhere.
pub const EVDEV_OFFSET: u32 = 8;

/// Opaque `struct xkb_context`.
#[repr(C)]
struct XkbContext {
    _opaque: [u8; 0],
}
/// Opaque `struct xkb_keymap`.
#[repr(C)]
struct XkbKeymap {
    _opaque: [u8; 0],
}
/// Opaque `struct xkb_state`.
#[repr(C)]
struct XkbState {
    _opaque: [u8; 0],
}

/// The C prototypes this module asserts about libxkbcommon.
///
/// Same convention as [`ffi::prototype`](crate::wayland::ffi) — the type is named once
/// and used both in [`Lib`] and in the `transmute` that fills it. Every one is
/// copied from `xkbcommon/xkbcommon.h` and is ABI-stable across libxkbcommon
/// 1.x.
mod prototype {
    use super::{XkbContext, XkbKeymap, XkbState, c_char, c_int};

    /// `struct xkb_context *xkb_context_new(enum xkb_context_flags)`
    pub type ContextNew = unsafe extern "C" fn(c_int) -> *mut XkbContext;
    /// `void xkb_context_unref(struct xkb_context *)`
    pub type ContextUnref = unsafe extern "C" fn(*mut XkbContext);
    /// `struct xkb_keymap *xkb_keymap_new_from_buffer(struct xkb_context *,
    /// const char *buffer, size_t length, enum xkb_keymap_format,
    /// enum xkb_keymap_compile_flags)`
    pub type KeymapNewFromBuffer =
        unsafe extern "C" fn(*mut XkbContext, *const c_char, usize, c_int, c_int) -> *mut XkbKeymap;
    /// `void xkb_keymap_unref(struct xkb_keymap *)`
    pub type KeymapUnref = unsafe extern "C" fn(*mut XkbKeymap);
    /// `xkb_mod_index_t xkb_keymap_mod_get_index(struct xkb_keymap *, const char *)`
    pub type KeymapModGetIndex = unsafe extern "C" fn(*mut XkbKeymap, *const c_char) -> u32;
    /// `int xkb_keymap_key_repeats(struct xkb_keymap *, xkb_keycode_t)`
    pub type KeymapKeyRepeats = unsafe extern "C" fn(*mut XkbKeymap, u32) -> c_int;
    /// `struct xkb_state *xkb_state_new(struct xkb_keymap *)`
    pub type StateNew = unsafe extern "C" fn(*mut XkbKeymap) -> *mut XkbState;
    /// `void xkb_state_unref(struct xkb_state *)`
    pub type StateUnref = unsafe extern "C" fn(*mut XkbState);
    /// `enum xkb_state_component xkb_state_update_mask(struct xkb_state *,
    /// xkb_mod_mask_t depressed, latched, locked, xkb_layout_index_t
    /// depressed_layout, latched_layout, locked_layout)`
    pub type StateUpdateMask =
        unsafe extern "C" fn(*mut XkbState, u32, u32, u32, u32, u32, u32) -> c_int;
    /// `xkb_keysym_t xkb_state_key_get_one_sym(struct xkb_state *, xkb_keycode_t)`
    pub type StateKeyGetOneSym = unsafe extern "C" fn(*mut XkbState, u32) -> u32;
    /// `int xkb_state_key_get_utf8(struct xkb_state *, xkb_keycode_t, char *, size_t)`
    pub type StateKeyGetUtf8 =
        unsafe extern "C" fn(*mut XkbState, u32, *mut c_char, usize) -> c_int;
    /// `int xkb_state_mod_index_is_active(struct xkb_state *, xkb_mod_index_t,
    /// enum xkb_state_component)`
    pub type StateModIndexIsActive = unsafe extern "C" fn(*mut XkbState, u32, c_int) -> c_int;
    /// `enum xkb_state_component xkb_state_update_key(struct xkb_state *,
    /// xkb_keycode_t, enum xkb_key_direction)`
    pub type StateUpdateKey = unsafe extern "C" fn(*mut XkbState, u32, c_int) -> c_int;
}

/// The C prototypes this module asserts about **libxkbcommon-x11**.
///
/// A second library, loaded separately, because it is separately packaged: a
/// Wayland-only machine has libxkbcommon and no `-x11`, and the X11 backend has
/// to degrade rather than refuse to start. The objects it returns are ordinary
/// `xkb_keymap`/`xkb_state` values freed with the plain libxkbcommon
/// destructors, which is why [`Keymap`] needs no second handle to drop them.
///
/// Copied from `xkbcommon/xkbcommon-x11.h`.
mod x11_prototype {
    use super::{XkbContext, XkbKeymap, XkbState, c_int, c_void};

    /// `int xkb_x11_setup_xkb_extension(xcb_connection_t *, uint16_t major,
    /// uint16_t minor, enum xkb_x11_setup_xkb_extension_flags, uint16_t
    /// *major_out, uint16_t *minor_out, uint8_t *base_event_out, uint8_t
    /// *base_error_out)`
    pub type SetupXkbExtension = unsafe extern "C" fn(
        *mut c_void,
        u16,
        u16,
        c_int,
        *mut u16,
        *mut u16,
        *mut u8,
        *mut u8,
    ) -> c_int;
    /// `int32_t xkb_x11_get_core_keyboard_device_id(xcb_connection_t *)`
    pub type GetCoreKeyboardDeviceId = unsafe extern "C" fn(*mut c_void) -> i32;
    /// `struct xkb_keymap *xkb_x11_keymap_new_from_device(struct xkb_context *,
    /// xcb_connection_t *, int32_t device_id, enum xkb_keymap_compile_flags)`
    pub type KeymapNewFromDevice =
        unsafe extern "C" fn(*mut XkbContext, *mut c_void, i32, c_int) -> *mut XkbKeymap;
    /// `struct xkb_state *xkb_x11_state_new_from_device(struct xkb_keymap *,
    /// xcb_connection_t *, int32_t device_id)`
    pub type StateNewFromDevice =
        unsafe extern "C" fn(*mut XkbKeymap, *mut c_void, i32) -> *mut XkbState;
}

/// Every libxkbcommon entry point these backends use.
///
/// Thirteen functions, all of them used; the same "audited by use" rule
/// [`ffi::Lib`](crate::wayland::ffi::Lib) states.
struct Lib {
    context_new: prototype::ContextNew,
    context_unref: prototype::ContextUnref,
    keymap_new_from_buffer: prototype::KeymapNewFromBuffer,
    keymap_unref: prototype::KeymapUnref,
    keymap_mod_get_index: prototype::KeymapModGetIndex,
    keymap_key_repeats: prototype::KeymapKeyRepeats,
    state_new: prototype::StateNew,
    state_unref: prototype::StateUnref,
    state_update_mask: prototype::StateUpdateMask,
    state_update_key: prototype::StateUpdateKey,
    state_key_get_one_sym: prototype::StateKeyGetOneSym,
    state_key_get_utf8: prototype::StateKeyGetUtf8,
    state_mod_index_is_active: prototype::StateModIndexIsActive,
}

/// Every libxkbcommon-x11 entry point the X11 backend uses.
struct X11Lib {
    setup_xkb_extension: x11_prototype::SetupXkbExtension,
    get_core_keyboard_device_id: x11_prototype::GetCoreKeyboardDeviceId,
    keymap_new_from_device: x11_prototype::KeymapNewFromDevice,
    state_new_from_device: x11_prototype::StateNewFromDevice,
}

/// The sonames tried, in order; see [`ffi::SONAMES`](crate::wayland::ffi).
const SONAMES: [&CStr; 2] = [c"libxkbcommon.so.0", c"libxkbcommon.so"];

/// As [`SONAMES`], for the X11 companion library.
const X11_SONAMES: [&CStr; 2] = [c"libxkbcommon-x11.so.0", c"libxkbcommon-x11.so"];

static LIB: OnceLock<Option<Lib>> = OnceLock::new();
static X11_LIB: OnceLock<Option<X11Lib>> = OnceLock::new();

fn load() -> Option<&'static Lib> {
    LIB.get_or_init(load_uncached).as_ref()
}

fn load_x11() -> Option<&'static X11Lib> {
    X11_LIB.get_or_init(load_x11_uncached).as_ref()
}

/// Whether libxkbcommon-x11 is available in this process.
///
/// Distinct from [`available`]: an X11 session can have libxkbcommon (every
/// GTK application pulls it in) and not its X11 companion, and the backend then
/// runs the degraded column of the [module docs](self) table rather than
/// failing.
#[must_use]
pub fn x11_available() -> bool {
    load().is_some() && load_x11().is_some()
}

/// Whether libxkbcommon is available in this process.
///
/// What [`ShellCaps::TEXT_IME`](crate::ShellCaps) is latched from.
#[must_use]
pub fn available() -> bool {
    load().is_some()
}

fn load_uncached() -> Option<Lib> {
    let mut handle = ptr::null_mut();
    for soname in SONAMES {
        // SAFETY: `soname` is a NUL-terminated literal and `RTLD_NOW` is a
        // valid flag. `dlopen` returns a handle or null and takes ownership of
        // nothing.
        handle = unsafe { dlopen(soname.as_ptr(), RTLD_NOW) };
        if !handle.is_null() {
            break;
        }
    }
    if handle.is_null() {
        log::warn!(
            "libxkbcommon is not available: keys still map to KeyCode, but there \
             will be no keysyms and no text input"
        );
        return None;
    }

    /// Resolves one symbol or gives up on the whole library.
    macro_rules! symbol {
        ($name:literal, $ty:ty) => {{
            // SAFETY: `handle` is a live `dlopen` handle and the name is a
            // NUL-terminated literal.
            let raw = unsafe { dlsym(handle, concat!($name, "\0").as_ptr().cast()) };
            if raw.is_null() {
                log::warn!("libxkbcommon has no symbol {}; degrading", $name);
                return None;
            }
            // SAFETY: `$ty` is the prototype `xkbcommon/xkbcommon.h` declares
            // for this symbol, and libxkbcommon's ABI is stable across 1.x.
            unsafe { core::mem::transmute::<*mut c_void, $ty>(raw) }
        }};
    }

    Some(Lib {
        context_new: symbol!("xkb_context_new", prototype::ContextNew),
        context_unref: symbol!("xkb_context_unref", prototype::ContextUnref),
        keymap_new_from_buffer: symbol!(
            "xkb_keymap_new_from_buffer",
            prototype::KeymapNewFromBuffer
        ),
        keymap_unref: symbol!("xkb_keymap_unref", prototype::KeymapUnref),
        keymap_mod_get_index: symbol!("xkb_keymap_mod_get_index", prototype::KeymapModGetIndex),
        keymap_key_repeats: symbol!("xkb_keymap_key_repeats", prototype::KeymapKeyRepeats),
        state_new: symbol!("xkb_state_new", prototype::StateNew),
        state_unref: symbol!("xkb_state_unref", prototype::StateUnref),
        state_update_mask: symbol!("xkb_state_update_mask", prototype::StateUpdateMask),
        state_update_key: symbol!("xkb_state_update_key", prototype::StateUpdateKey),
        state_key_get_one_sym: symbol!("xkb_state_key_get_one_sym", prototype::StateKeyGetOneSym),
        state_key_get_utf8: symbol!("xkb_state_key_get_utf8", prototype::StateKeyGetUtf8),
        state_mod_index_is_active: symbol!(
            "xkb_state_mod_index_is_active",
            prototype::StateModIndexIsActive
        ),
    })
}

fn load_x11_uncached() -> Option<X11Lib> {
    let mut handle = ptr::null_mut();
    for soname in X11_SONAMES {
        // SAFETY: `soname` is a NUL-terminated literal and `RTLD_NOW` is a
        // valid flag. `dlopen` returns a handle or null and takes ownership of
        // nothing.
        handle = unsafe { dlopen(soname.as_ptr(), RTLD_NOW) };
        if !handle.is_null() {
            break;
        }
    }
    if handle.is_null() {
        log::warn!(
            "libxkbcommon-x11 is not available: keys still map to KeyCode, but there \
             will be no keysyms and no text input on X11"
        );
        return None;
    }

    /// Resolves one symbol or gives up on the whole library.
    macro_rules! symbol {
        ($name:literal, $ty:ty) => {{
            // SAFETY: `handle` is a live `dlopen` handle and the name is a
            // NUL-terminated literal.
            let raw = unsafe { dlsym(handle, concat!($name, "\0").as_ptr().cast()) };
            if raw.is_null() {
                log::warn!("libxkbcommon-x11 has no symbol {}; degrading", $name);
                return None;
            }
            // SAFETY: `$ty` is the prototype `xkbcommon/xkbcommon-x11.h`
            // declares for this symbol, and the ABI is stable across 1.x.
            unsafe { core::mem::transmute::<*mut c_void, $ty>(raw) }
        }};
    }

    Some(X11Lib {
        setup_xkb_extension: symbol!(
            "xkb_x11_setup_xkb_extension",
            x11_prototype::SetupXkbExtension
        ),
        get_core_keyboard_device_id: symbol!(
            "xkb_x11_get_core_keyboard_device_id",
            x11_prototype::GetCoreKeyboardDeviceId
        ),
        keymap_new_from_device: symbol!(
            "xkb_x11_keymap_new_from_device",
            x11_prototype::KeymapNewFromDevice
        ),
        state_new_from_device: symbol!(
            "xkb_x11_state_new_from_device",
            x11_prototype::StateNewFromDevice
        ),
    })
}

/// The XKB modifier names this engine's [`Modifiers`] bits correspond to.
///
/// Names rather than bit positions, because a keymap decides for itself which
/// real modifier a virtual one maps onto — `Alt` is `Mod1` on nearly every
/// layout and is not required to be. Looking the index up by name once per
/// keymap is the only correct way to read the mask.
///
/// `ISO_Level3_Shift` (AltGr) is `Mod5` on every layout that has it. It is kept
/// separate from [`Modifiers::ALT`] deliberately — see that type's docs for the
/// `Alt+E` versus `€` collision it exists to prevent.
const MODIFIER_NAMES: [(&CStr, Modifiers); 7] = [
    (c"Shift", Modifiers::SHIFT),
    (c"Control", Modifiers::CTRL),
    (c"Mod1", Modifiers::ALT),
    (c"Mod4", Modifiers::SUPER),
    (c"Lock", Modifiers::CAPS_LOCK),
    (c"Mod2", Modifiers::NUM_LOCK),
    (c"Mod5", Modifiers::ALT_GR),
];

/// A compiled keymap and the state tracking which modifiers are active on it.
///
/// One per `wl_keyboard` on Wayland, one per connection on X11 (there is one
/// core keyboard, and per-device keymaps are XInput2 territory the seam has no
/// vocabulary for). Replaced wholesale when the layout changes — a
/// `wl_keyboard.keymap` event on Wayland, a `MappingNotify` on X11.
pub struct Keymap {
    lib: &'static Lib,
    context: *mut XkbContext,
    keymap: *mut XkbKeymap,
    state: *mut XkbState,
    /// `(xkb modifier index, engine bit)` for the modifiers this keymap has.
    /// A modifier the keymap does not define is absent rather than always-off,
    /// which is the same thing here but costs one fewer call per event.
    modifiers: Vec<(u32, Modifiers)>,
}

impl core::fmt::Debug for Keymap {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Keymap")
            .field("modifiers", &self.modifiers.len())
            .finish()
    }
}

impl Keymap {
    /// Compiles the keymap the compositor sent over `fd`.
    ///
    /// **Takes ownership of `fd` and always closes it**, including on every
    /// failure path. A `wl_keyboard.keymap` whose descriptor is leaked leaks one
    /// per layout switch, and the compositor cannot reclaim the memory behind
    /// it.
    ///
    /// Returns `None` when libxkbcommon is unavailable, the mapping fails, or
    /// the keymap does not compile — all of which degrade to the no-keysym
    /// column of the [module docs](self) rather than to an error.
    #[must_use]
    pub fn from_fd(fd: i32, size: u32) -> Option<Self> {
        if fd < 0 {
            // The negative value libwayland yields for a malformed message.
            // There is nothing to close.
            return None;
        }
        // Adopting the descriptor is what closes it: the mapping outlives the
        // descriptor (`mmap` keeps its own reference), so the file may be
        // dropped as soon as `compile` returns, on every path including the
        // early `None`s inside it.
        //
        // SAFETY: `fd` was created by libwayland's own `recvmsg` of an
        // `SCM_RIGHTS` message and handed to the dispatcher, which transfers
        // ownership to the client. Every call site takes the value out of the
        // decoded event and never reads it again, so this adopts it exactly
        // once and no other owner can close it.
        let file = unsafe { std::fs::File::from_raw_fd(fd) };

        // `size` is the compositor's word for how long the keymap is, and
        // `mmap` happily maps a length past the end of the file it is given —
        // touching those pages then raises **SIGBUS**, which is a crash and not
        // a failure this function could return `None` for. So the file is asked
        // how big it actually is, and a compositor whose two answers disagree
        // gets the no-keysym degradation the module documents rather than
        // taking the process down.
        let declared = size as usize;
        let actual = file
            .metadata()
            .ok()
            .and_then(|metadata| usize::try_from(metadata.len()).ok())?;
        if actual < declared {
            log::warn!(
                "the compositor declared a {declared}-byte keymap but sent a \
                 {actual}-byte file; ignoring it"
            );
            return None;
        }
        Self::compile(file.as_raw_fd(), declared)
    }

    /// Reads the keymap the X server holds for the core keyboard.
    ///
    /// The X11 half of the [module docs](self) table. There is no descriptor
    /// and no buffer: libxkbcommon-x11 asks the server for the keymap over the
    /// *same* `xcb_connection_t` the backend is already using, which is why
    /// this takes a connection pointer rather than bytes.
    ///
    /// Returns `None` when either library is missing, when the server has no
    /// XKB extension (a 1990s X server, or a proxy that filters it), or when
    /// the keymap does not compile. Every one of those degrades to the
    /// no-keysym column rather than to an error, exactly as the Wayland path
    /// does.
    ///
    /// # Safety
    ///
    /// `connection` must be a live `xcb_connection_t*` with no error latched.
    /// libxkbcommon-x11 issues synchronous round trips on it, so it must not be
    /// used concurrently from another thread — which a [`Shell`](crate::Shell)
    /// already forbids by not being `Send`.
    #[must_use]
    pub unsafe fn from_x11_device(connection: *mut c_void) -> Option<Self> {
        let lib = load()?;
        let x11 = load_x11()?;

        // `xkb_x11_setup_xkb_extension` must succeed before anything else in
        // the library may be called; it is what negotiates the XKB version on
        // this connection. 1.0 is the minimum libxkbcommon itself asks for.
        // SAFETY: the caller guarantees a live connection; every out-parameter
        // is a live local, and `0` is `XKB_X11_SETUP_XKB_EXTENSION_NO_FLAGS`.
        let ok = unsafe {
            (x11.setup_xkb_extension)(
                connection,
                1,
                0,
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        if ok == 0 {
            log::warn!("the X server has no usable XKB extension; no keysyms this session");
            return None;
        }
        // SAFETY: as above, and the extension is now set up.
        let device = unsafe { (x11.get_core_keyboard_device_id)(connection) };
        if device == -1 {
            log::warn!("the X server reports no core keyboard device");
            return None;
        }

        // SAFETY: a null context flag set is `XKB_CONTEXT_NO_FLAGS`, and every
        // pointer is checked before use. Each object is released exactly once —
        // here on the failure paths, or in `Drop` on success.
        unsafe {
            let context = (lib.context_new)(0);
            if context.is_null() {
                return None;
            }
            let keymap = (x11.keymap_new_from_device)(context, connection, device, 0);
            if keymap.is_null() {
                (lib.context_unref)(context);
                log::warn!("the X server's keymap did not compile; no keysyms this session");
                return None;
            }
            // `state_new_from_device` rather than `state_new`: it seeds the
            // state with the modifiers and layout group that are *already*
            // latched on the server, so a session started with Caps Lock on
            // does not think it is off until the key is pressed.
            let state = (x11.state_new_from_device)(keymap, connection, device);
            if state.is_null() {
                (lib.keymap_unref)(keymap);
                (lib.context_unref)(context);
                return None;
            }
            Some(Self {
                lib,
                context,
                keymap,
                state,
                modifiers: Self::modifier_indices(lib, keymap),
            })
        }
    }

    /// `(xkb modifier index, engine bit)` for every modifier this keymap has.
    ///
    /// # Safety
    ///
    /// `keymap` must be live.
    unsafe fn modifier_indices(lib: &'static Lib, keymap: *mut XkbKeymap) -> Vec<(u32, Modifiers)> {
        MODIFIER_NAMES
            .iter()
            .filter_map(|(name, bit)| {
                // SAFETY: the caller guarantees `keymap` is live, and the names
                // are NUL-terminated literals.
                let index = unsafe { (lib.keymap_mod_get_index)(keymap, name.as_ptr()) };
                (index != MOD_INVALID).then_some((index, *bit))
            })
            .collect()
    }

    fn compile(fd: i32, size: usize) -> Option<Self> {
        let lib = load()?;
        if size == 0 {
            return None;
        }
        // SAFETY: `fd` is the readable descriptor the compositor sent, and
        // `size` has been checked against the file's own length by
        // [`from_fd`], so every page this maps is backed — which is what keeps
        // the read of the last byte below from raising SIGBUS.
        // `MAP_PRIVATE | PROT_READ` is what `wl_keyboard.keymap` documents as
        // the only permitted mapping.
        let mapped = unsafe { mmap(ptr::null_mut(), size, PROT_READ, MAP_PRIVATE, fd, 0) };
        if mapped == MAP_FAILED || mapped.is_null() {
            log::warn!("cannot mmap the compositor's keymap ({size} bytes)");
            return None;
        }
        // The compositor NUL-terminates the keymap and counts the terminator in
        // `size`; `xkb_keymap_new_from_buffer` wants the length *without* it,
        // and mis-stating it by one byte is a parse error on some keymaps.
        // SAFETY: the mapping covers `size` bytes, so reading the last one is
        // in bounds.
        let text_len = if unsafe { *mapped.cast::<u8>().add(size - 1) } == 0 {
            size - 1
        } else {
            size
        };

        // SAFETY: a null context flag set is `XKB_CONTEXT_NO_FLAGS`. The
        // context, keymap and state are each checked for null before use, and
        // each is released exactly once — here on the failure paths, or in
        // `Drop` on success.
        let compiled = unsafe {
            let context = (lib.context_new)(0);
            if context.is_null() {
                return Self::unmap(mapped, size, None);
            }
            let keymap = (lib.keymap_new_from_buffer)(
                context,
                mapped.cast::<c_char>(),
                text_len,
                KEYMAP_FORMAT_TEXT_V1,
                0,
            );
            if keymap.is_null() {
                (lib.context_unref)(context);
                log::warn!("the compositor's keymap did not compile; no keysyms this session");
                return Self::unmap(mapped, size, None);
            }
            let state = (lib.state_new)(keymap);
            if state.is_null() {
                (lib.keymap_unref)(keymap);
                (lib.context_unref)(context);
                return Self::unmap(mapped, size, None);
            }
            Self {
                lib,
                context,
                keymap,
                state,
                modifiers: Self::modifier_indices(lib, keymap),
            }
        };
        Self::unmap(mapped, size, Some(compiled))
    }

    /// Releases the mapping and forwards `result`.
    ///
    /// libxkbcommon copies the text during compilation, so the mapping is dead
    /// the moment `xkb_keymap_new_from_buffer` returns.
    fn unmap(mapped: *mut c_void, size: usize, result: Option<Self>) -> Option<Self> {
        // SAFETY: `mapped`/`size` are exactly what the `mmap` above returned,
        // and nothing holds a reference into the region — libxkbcommon copied
        // what it needed.
        unsafe { munmap(mapped, size) };
        result
    }

    /// Applies a `wl_keyboard.modifiers` event.
    pub fn update(&mut self, depressed: u32, latched: u32, locked: u32, group: u32) {
        // SAFETY: `self.state` is live for this value's lifetime; the masks are
        // opaque integers XKB validates itself.
        unsafe {
            (self.lib.state_update_mask)(self.state, depressed, latched, locked, 0, 0, group);
        }
    }

    /// Applies one key transition, for a backend with no modifier event.
    ///
    /// # Why X11 needs this and Wayland does not
    ///
    /// Wayland has `wl_keyboard.modifiers`, an authoritative message carrying
    /// the four masks XKB wants, sent whenever any of them changes. X11 has no
    /// such event outside the XKB extension's own `StateNotify` — what it has
    /// instead is a 16-bit `state` field stapled to every key, button and
    /// motion event, which is a *flattened* view: it cannot distinguish
    /// depressed from latched, it is sampled **before** the key in the same
    /// event is applied, and it has no room for the group beyond two bits.
    ///
    /// So the X11 backend drives the state the way libxkbcommon's own
    /// documentation recommends when no `StateNotify` is available: feed it
    /// every key transition and let it do the modifier bookkeeping, which is
    /// the only path that gets latches, locks and level-3 shifts right.
    /// [`resync_from_x11_state`](Self::resync_from_x11_state) repairs the one
    /// thing this cannot see — transitions that happened while another window
    /// had focus.
    ///
    /// `keycode` is the **X11** keycode, i.e. evdev plus
    /// [`EVDEV_OFFSET`]; that is already XKB's own numbering, so it is passed
    /// through unchanged.
    pub fn update_key(&mut self, keycode: u32, pressed: bool) {
        /// `XKB_KEY_UP`.
        const UP: c_int = 0;
        /// `XKB_KEY_DOWN`.
        const DOWN: c_int = 1;
        // SAFETY: `self.state` is live; an out-of-range keycode is a no-op
        // inside libxkbcommon rather than a fault.
        unsafe {
            (self.lib.state_update_key)(self.state, keycode, if pressed { DOWN } else { UP });
        }
    }

    /// Re-seeds the state from an X11 event's `state` mask and layout group.
    ///
    /// Called on `FocusIn`, and only there. While another client had focus this
    /// process saw none of its key events, so a Shift pressed and released
    /// elsewhere would still read as held — the classic "stuck modifier after
    /// alt-tab" bug. The event that *restores* focus carries the server's own
    /// view of the modifiers, which is the authority, so focus is exactly the
    /// moment to throw away the incrementally-tracked state.
    ///
    /// The mask is flattened, so this is lossy in a way
    /// [`update_key`](Self::update_key) is not: everything that is not a lock
    /// is reported as depressed. That is the correct approximation — a latch
    /// misread as a press is released by the very next key, whereas a lock
    /// misread as a press would stick.
    pub fn resync_from_x11_state(&mut self, state: u16) {
        /// `LockMask` (Caps Lock) and `Mod2Mask` (Num Lock) — the two bits in
        /// an X11 event mask that mean *locked* rather than *held*.
        const LOCKED: u16 = (1 << 1) | (1 << 4);
        /// The two bits X11 reserves for the keyboard group, as XKB defines
        /// them in `XkbGroupForCoreState`.
        const GROUP_SHIFT: u16 = 13;
        let group = u32::from((state >> GROUP_SHIFT) & 0x3);
        let depressed = u32::from(state & !LOCKED & 0xff);
        let locked = u32::from(state & LOCKED);
        self.update(depressed, 0, locked, group);
    }

    /// The engine modifiers currently in effect.
    #[must_use]
    pub fn modifiers(&self) -> Modifiers {
        let mut active = Modifiers::empty();
        for (index, bit) in &self.modifiers {
            // SAFETY: `self.state` is live and `index` came from this keymap's
            // own `xkb_keymap_mod_get_index`.
            let is_active = unsafe {
                (self.lib.state_mod_index_is_active)(self.state, *index, STATE_MODS_EFFECTIVE)
            };
            if is_active > 0 {
                active |= *bit;
            }
        }
        active
    }

    /// The symbol `scancode` (an evdev code) produces in the current layout and
    /// modifier state.
    #[must_use]
    pub fn keysym(&self, scancode: u32) -> Keysym {
        // SAFETY: `self.state` is live; an out-of-range keycode yields
        // `XKB_KEY_NoSymbol` rather than misbehaving, which is why no range
        // check is needed here.
        Keysym(unsafe { (self.lib.state_key_get_one_sym)(self.state, scancode + EVDEV_OFFSET) })
    }

    /// The text `scancode` commits in the current layout, or `None` when it
    /// commits nothing a text field should receive.
    ///
    /// Control characters are filtered out — Enter, Tab, Escape and Backspace
    /// all produce one, and a text field wants those as
    /// [`ShellEvent::Key`](crate::ShellEvent::Key) so it can move a cursor,
    /// not as a literal `\r` in its buffer. Everything printable, including
    /// whatever a dead-key sequence resolves to, is passed through.
    #[must_use]
    pub fn text(&self, scancode: u32) -> Option<String> {
        // Big enough for anything a single key commits; XKB's own examples use
        // 32. The call NUL-terminates and returns the length it *wanted*, so a
        // longer result is detected rather than truncated silently.
        let mut buffer = [0u8; 64];
        // SAFETY: `self.state` is live and `buffer` is a writable array of
        // exactly the length passed, which is `xkb_state_key_get_utf8`'s
        // contract.
        let written = unsafe {
            (self.lib.state_key_get_utf8)(
                self.state,
                scancode + EVDEV_OFFSET,
                buffer.as_mut_ptr().cast::<c_char>(),
                buffer.len(),
            )
        };
        let written = usize::try_from(written).ok()?;
        if written == 0 || written >= buffer.len() {
            return None;
        }
        let text = core::str::from_utf8(&buffer[..written]).ok()?;
        if text.is_empty() || text.chars().all(char::is_control) {
            return None;
        }
        Some(text.to_string())
    }

    /// Whether the keymap says this key auto-repeats.
    ///
    /// Modifiers, Caps Lock and the function-lock keys do not, and synthesizing
    /// repeats for them would hold Shift down forever from the action layer's
    /// point of view.
    #[must_use]
    pub fn repeats(&self, scancode: u32) -> bool {
        // SAFETY: `self.keymap` is live; an out-of-range keycode answers 0.
        unsafe { (self.lib.keymap_key_repeats)(self.keymap, scancode + EVDEV_OFFSET) > 0 }
    }
}

impl Drop for Keymap {
    fn drop(&mut self) {
        // SAFETY: each pointer was returned live by the matching constructor
        // and is released exactly once, innermost first — a state holds a
        // reference to its keymap, and a keymap to its context.
        unsafe {
            (self.lib.state_unref)(self.state);
            (self.lib.keymap_unref)(self.keymap);
            (self.lib.context_unref)(self.context);
        }
    }
}

/// The modifiers implied by a set of held keys, for the no-libxkbcommon path.
///
/// Pure, so the degraded path is testable without uninstalling a system
/// library. It cannot see latched state — Caps Lock reads as a key that is
/// down, not as a lock that is on — which is exactly the loss the [module
/// docs](self) table records.
#[must_use]
pub fn modifiers_from_held(held: &[KeyCode]) -> Modifiers {
    let mut active = Modifiers::empty();
    for key in held {
        active |= match key {
            KeyCode::ShiftLeft | KeyCode::ShiftRight => Modifiers::SHIFT,
            KeyCode::ControlLeft | KeyCode::ControlRight => Modifiers::CTRL,
            KeyCode::AltLeft => Modifiers::ALT,
            // Right Alt is AltGr on every layout that has one, and plain Alt on
            // US-QWERTY. Reporting both bits is the only answer that is never
            // silently wrong in the degraded path, and it is why the real path
            // asks XKB instead.
            KeyCode::AltRight => Modifiers::ALT | Modifiers::ALT_GR,
            KeyCode::SuperLeft | KeyCode::SuperRight => Modifiers::SUPER,
            _ => Modifiers::empty(),
        };
    }
    active
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_degraded_modifier_path_covers_every_chord_bit() {
        assert_eq!(modifiers_from_held(&[]), Modifiers::empty());
        assert_eq!(
            modifiers_from_held(&[KeyCode::ControlLeft, KeyCode::ShiftRight]),
            Modifiers::CTRL | Modifiers::SHIFT
        );
        assert_eq!(modifiers_from_held(&[KeyCode::SuperLeft]), Modifiers::SUPER);
        // The one honest ambiguity: without a keymap, right Alt is both.
        assert_eq!(
            modifiers_from_held(&[KeyCode::AltRight]),
            Modifiers::ALT | Modifiers::ALT_GR
        );
        assert_eq!(modifiers_from_held(&[KeyCode::AltLeft]), Modifiers::ALT);
        // Keys that are not modifiers contribute nothing.
        assert_eq!(
            modifiers_from_held(&[KeyCode::KeyW, KeyCode::Space]),
            Modifiers::empty()
        );
        assert_eq!(
            modifiers_from_held(&[KeyCode::ControlLeft, KeyCode::KeyS]).chord(),
            Modifiers::CTRL
        );
    }

    #[test]
    fn the_evdev_offset_is_the_x11_one() {
        // Wayland sends evdev codes; XKB numbers them eight higher. A backend
        // that forgot this reports `KeyQ` as `Digit4`, which looks like a
        // keyboard layout problem and is not.
        assert_eq!(EVDEV_OFFSET, 8);
        // KEY_A is 30 in evdev, 38 in XKB — the numbers in every keymap file.
        assert_eq!(30 + EVDEV_OFFSET, 38);
    }

    #[test]
    fn the_modifier_name_table_covers_the_engine_bits_that_are_layout_defined() {
        let covered = MODIFIER_NAMES
            .iter()
            .fold(Modifiers::empty(), |set, (_, bit)| set | *bit);
        assert_eq!(
            covered,
            Modifiers::all(),
            "every Modifiers bit must have an XKB name to read it from"
        );
        let mut names: Vec<&CStr> = MODIFIER_NAMES.iter().map(|(name, _)| *name).collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "two bits must not read the same index");
    }

    #[test]
    fn loading_is_idempotent_and_never_panics() {
        // The property the whole `dlopen` decision exists for: this must be an
        // ordinary boolean on a machine with libxkbcommon and on one without.
        let first = available();
        assert_eq!(first, available(), "the load is cached, not repeated");
    }
}
