//! libxkbcommon's compose module: dead keys and Compose-key sequences, fed
//! by [`Keymap::text`](super::Keymap::text). What it does is in the [parent
//! module's docs](super#compose); this is the binding.

use core::ffi::{c_char, c_int, c_void};

use crcbl_core::input::Keysym;

use super::{Lib, XkbContext, dlsym, printable};

/// Opaque `struct xkb_compose_table`.
#[repr(C)]
struct XkbComposeTable {
    _opaque: [u8; 0],
}
/// Opaque `struct xkb_compose_state`.
#[repr(C)]
struct XkbComposeState {
    _opaque: [u8; 0],
}

/// `XKB_COMPOSE_FEED_IGNORED`: the keysym is a modifier, which a sequence
/// passes over.
const COMPOSE_FEED_IGNORED: c_int = 0;
/// `XKB_COMPOSE_COMPOSING`.
const COMPOSE_COMPOSING: c_int = 1;
/// `XKB_COMPOSE_COMPOSED`.
const COMPOSE_COMPOSED: c_int = 2;
/// `XKB_COMPOSE_CANCELLED`.
const COMPOSE_CANCELLED: c_int = 3;

/// The C prototypes this module asserts about libxkbcommon's compose module,
/// copied from `xkbcommon/xkbcommon-compose.h`. It lives in libxkbcommon itself
/// and has since 0.5.
mod compose_prototype {
    use super::{XkbComposeState, XkbComposeTable, XkbContext, c_char, c_int};

    /// `struct xkb_compose_table *xkb_compose_table_new_from_locale(struct
    /// xkb_context *, const char *locale, enum xkb_compose_compile_flags)`
    pub type TableNewFromLocale =
        unsafe extern "C" fn(*mut XkbContext, *const c_char, c_int) -> *mut XkbComposeTable;
    /// `struct xkb_compose_table *xkb_compose_table_new_from_buffer(struct
    /// xkb_context *, const char *buffer, size_t length, const char *locale,
    /// enum xkb_compose_format, enum xkb_compose_compile_flags)`
    #[cfg(test)]
    pub type TableNewFromBuffer = unsafe extern "C" fn(
        *mut XkbContext,
        *const c_char,
        usize,
        *const c_char,
        c_int,
        c_int,
    ) -> *mut XkbComposeTable;
    /// `void xkb_compose_table_unref(struct xkb_compose_table *)`
    pub type TableUnref = unsafe extern "C" fn(*mut XkbComposeTable);
    /// `struct xkb_compose_state *xkb_compose_state_new(struct
    /// xkb_compose_table *, enum xkb_compose_state_flags)`
    pub type StateNew = unsafe extern "C" fn(*mut XkbComposeTable, c_int) -> *mut XkbComposeState;
    /// `void xkb_compose_state_unref(struct xkb_compose_state *)`
    pub type StateUnref = unsafe extern "C" fn(*mut XkbComposeState);
    /// `enum xkb_compose_feed_result xkb_compose_state_feed(struct
    /// xkb_compose_state *, xkb_keysym_t)`
    pub type StateFeed = unsafe extern "C" fn(*mut XkbComposeState, u32) -> c_int;
    /// `void xkb_compose_state_reset(struct xkb_compose_state *)`
    pub type StateReset = unsafe extern "C" fn(*mut XkbComposeState);
    /// `enum xkb_compose_status xkb_compose_state_get_status(struct
    /// xkb_compose_state *)`
    pub type StateGetStatus = unsafe extern "C" fn(*mut XkbComposeState) -> c_int;
    /// `int xkb_compose_state_get_utf8(struct xkb_compose_state *, char
    /// *buffer, size_t size)`
    pub type StateGetUtf8 = unsafe extern "C" fn(*mut XkbComposeState, *mut c_char, usize) -> c_int;
}

/// Every compose entry point [`Compose`] uses, resolved from the same library
/// as [`Lib`] but apart from it: a library without them still has keysyms and
/// text, only no dead keys.
pub(super) struct ComposeLib {
    table_new_from_locale: compose_prototype::TableNewFromLocale,
    /// Only the tests build a table from text, so a sequence can be checked
    /// without depending on the machine's locale files.
    #[cfg(test)]
    table_new_from_buffer: compose_prototype::TableNewFromBuffer,
    table_unref: compose_prototype::TableUnref,
    state_new: compose_prototype::StateNew,
    state_unref: compose_prototype::StateUnref,
    state_feed: compose_prototype::StateFeed,
    state_reset: compose_prototype::StateReset,
    state_get_status: compose_prototype::StateGetStatus,
    state_get_utf8: compose_prototype::StateGetUtf8,
}

/// The compose module's entry points from `handle`, or `None` if any is
/// missing.
pub(super) fn load_compose(handle: *mut c_void) -> Option<ComposeLib> {
    /// Resolves one symbol or gives up on the compose module.
    macro_rules! symbol {
        ($name:literal, $ty:ty) => {{
            // SAFETY: `handle` is a live `dlopen` handle and the name is a
            // NUL-terminated literal.
            let raw = unsafe { dlsym(handle, concat!($name, "\0").as_ptr().cast()) };
            if raw.is_null() {
                crcbl_core::log::warn!(
                    "libxkbcommon has no symbol {}; dead keys will type nothing",
                    $name
                );
                return None;
            }
            // SAFETY: `$ty` is the prototype `xkbcommon/xkbcommon-compose.h`
            // declares for this symbol, and the ABI is stable across 1.x.
            unsafe { core::mem::transmute::<*mut c_void, $ty>(raw) }
        }};
    }

    Some(ComposeLib {
        table_new_from_locale: symbol!(
            "xkb_compose_table_new_from_locale",
            compose_prototype::TableNewFromLocale
        ),
        #[cfg(test)]
        table_new_from_buffer: symbol!(
            "xkb_compose_table_new_from_buffer",
            compose_prototype::TableNewFromBuffer
        ),
        table_unref: symbol!("xkb_compose_table_unref", compose_prototype::TableUnref),
        state_new: symbol!("xkb_compose_state_new", compose_prototype::StateNew),
        state_unref: symbol!("xkb_compose_state_unref", compose_prototype::StateUnref),
        state_feed: symbol!("xkb_compose_state_feed", compose_prototype::StateFeed),
        state_reset: symbol!("xkb_compose_state_reset", compose_prototype::StateReset),
        state_get_status: symbol!(
            "xkb_compose_state_get_status",
            compose_prototype::StateGetStatus
        ),
        state_get_utf8: symbol!(
            "xkb_compose_state_get_utf8",
            compose_prototype::StateGetUtf8
        ),
    })
}

/// What one pressed keysym did to the compose sequence.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Composed {
    /// It is no part of a sequence: the key commits its own text.
    Passed,
    /// It started or continued a sequence, or broke one: nothing is
    /// committed.
    Swallowed,
    /// It finished a sequence, which commits this text, if it is printable.
    Text(Option<String>),
}

/// A compose table and the sequence in progress against it.
pub(super) struct Compose {
    lib: &'static ComposeLib,
    table: *mut XkbComposeTable,
    state: *mut XkbComposeState,
}

impl Compose {
    /// The compose table for the user's locale, or `None` when libxkbcommon
    /// has no compose module or finds no table.
    ///
    /// # Safety
    ///
    /// `context` must be live. The table takes its own reference to it.
    pub(super) unsafe fn for_locale(lib: &'static Lib, context: *mut XkbContext) -> Option<Self> {
        let compose = lib.compose.as_ref()?;
        let locale = locale(std::env::var_os);
        // SAFETY: the caller guarantees a live context, the locale is a
        // NUL-terminated string that outlives the call, and `0` is
        // `XKB_COMPOSE_COMPILE_NO_FLAGS`.
        let table = unsafe { (compose.table_new_from_locale)(context, locale.as_ptr(), 0) };
        if table.is_null() {
            crcbl_core::log::warn!(
                "no compose table for the locale {locale:?}; dead keys will type nothing"
            );
            return None;
        }
        // SAFETY: a table just made, released here or in `Drop`.
        unsafe { Self::with_table(compose, table) }
    }

    /// A compose state over `table`, whose reference it takes over.
    ///
    /// # Safety
    ///
    /// `table` must be a live table this call may release.
    unsafe fn with_table(lib: &'static ComposeLib, table: *mut XkbComposeTable) -> Option<Self> {
        // SAFETY: the caller guarantees a live table; `0` is
        // `XKB_COMPOSE_STATE_NO_FLAGS`. The state holds its own reference to
        // the table, and this value keeps ours until `Drop`.
        let state = unsafe { (lib.state_new)(table, 0) };
        if state.is_null() {
            // SAFETY: ours to release, and nothing else holds it.
            unsafe { (lib.table_unref)(table) };
            return None;
        }
        Some(Self { lib, table, state })
    }

    /// Feeds one pressed keysym to the sequence.
    pub(super) fn feed(&mut self, keysym: Keysym) -> Composed {
        // SAFETY: `self.state` is live for this value's lifetime.
        let fed = unsafe { (self.lib.state_feed)(self.state, keysym.0) };
        if fed == COMPOSE_FEED_IGNORED {
            return Composed::Passed;
        }
        // SAFETY: as above.
        match unsafe { (self.lib.state_get_status)(self.state) } {
            COMPOSE_COMPOSING | COMPOSE_CANCELLED => Composed::Swallowed,
            COMPOSE_COMPOSED => {
                // SAFETY: as above, and `printable` passes a writable buffer
                // of exactly the length it names.
                let text = printable(|buffer, length| unsafe {
                    (self.lib.state_get_utf8)(self.state, buffer, length)
                });
                self.reset();
                Composed::Text(text)
            }
            _ => Composed::Passed,
        }
    }

    /// Abandons the sequence in progress.
    pub(super) fn reset(&mut self) {
        // SAFETY: `self.state` is live for this value's lifetime.
        unsafe { (self.lib.state_reset)(self.state) };
    }
}

impl Drop for Compose {
    fn drop(&mut self) {
        // SAFETY: each pointer is live and released exactly once, the state
        // first, since it holds a reference to the table.
        unsafe {
            (self.lib.state_unref)(self.state);
            (self.lib.table_unref)(self.table);
        }
    }
}

/// The locale a compose table is looked up for: the first of `LC_ALL`,
/// `LC_CTYPE` and `LANG` that `var` finds set and not empty, which is the
/// order libxkbcommon's documentation gives, or `C`.
fn locale(var: impl Fn(&'static str) -> Option<std::ffi::OsString>) -> std::ffi::CString {
    use std::os::unix::ffi::OsStringExt;
    ["LC_ALL", "LC_CTYPE", "LANG"]
        .into_iter()
        .filter_map(var)
        .find(|value| !value.is_empty())
        .and_then(|value| std::ffi::CString::new(value.into_vec()).ok())
        .unwrap_or_else(|| c"C".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linux::xkb::load;

    /// A compose table built from `text`, through the library CI installs
    /// for this job; the locale only names the character set.
    fn compose_from(text: &str) -> Compose {
        let lib = load().expect("libxkbcommon, which CI installs for the unit tests");
        let compose = lib.compose.as_ref().expect("libxkbcommon's compose module");
        // SAFETY: each object is checked and released: the context here, the
        // table by the `Compose` that takes it over.
        unsafe {
            let context = (lib.context_new)(0);
            assert!(!context.is_null());
            /// `XKB_COMPOSE_FORMAT_TEXT_V1`.
            const FORMAT_TEXT_V1: c_int = 1;
            let table = (compose.table_new_from_buffer)(
                context,
                text.as_ptr().cast(),
                text.len(),
                c"en_US.UTF-8".as_ptr(),
                FORMAT_TEXT_V1,
                0,
            );
            (lib.context_unref)(context);
            assert!(!table.is_null(), "the compose text parses");
            Compose::with_table(compose, table).expect("a compose state")
        }
    }

    /// **A dead key commits nothing, and the key after it commits the pair's
    /// text**; a key outside every sequence commits its own, a modifier in the
    /// middle of one is passed over, and a key that breaks a sequence commits
    /// nothing. Keysyms are X11's, from `xkbcommon-keysyms.h`.
    #[test]
    fn a_dead_key_composes_with_the_next_and_a_wrong_one_cancels() {
        const DEAD_ACUTE: Keysym = Keysym(0xfe51);
        const SHIFT_L: Keysym = Keysym(0xffe1);
        const E: Keysym = Keysym(0x65);
        const Q: Keysym = Keysym(0x71);
        let mut compose = compose_from("<dead_acute> <e> : \"é\" eacute\n");

        assert_eq!(compose.feed(E), Composed::Passed);
        assert_eq!(compose.feed(DEAD_ACUTE), Composed::Swallowed);
        assert_eq!(compose.feed(SHIFT_L), Composed::Passed);
        assert_eq!(compose.feed(E), Composed::Text(Some("é".to_string())));
        assert_eq!(
            compose.feed(E),
            Composed::Passed,
            "the sequence starts over"
        );

        assert_eq!(compose.feed(DEAD_ACUTE), Composed::Swallowed);
        assert_eq!(
            compose.feed(Q),
            Composed::Swallowed,
            "no sequence: cancelled"
        );
        assert_eq!(compose.feed(E), Composed::Passed);

        assert_eq!(compose.feed(DEAD_ACUTE), Composed::Swallowed);
        compose.reset();
        assert_eq!(
            compose.feed(E),
            Composed::Passed,
            "a reset forgets the dead key"
        );
    }

    /// The locale is the first of `LC_ALL`, `LC_CTYPE` and `LANG` that is set
    /// to something, or `C` when none is.
    #[test]
    fn the_locale_is_the_first_variable_set_to_something() {
        let from = |pairs: &'static [(&'static str, &'static str)]| {
            locale(move |name| {
                pairs
                    .iter()
                    .find(|(key, _)| *key == name)
                    .map(|(_, value)| std::ffi::OsString::from(value))
            })
        };
        assert_eq!(from(&[]).as_c_str(), c"C");
        assert_eq!(from(&[("LANG", "de_DE.UTF-8")]).as_c_str(), c"de_DE.UTF-8");
        assert_eq!(
            from(&[("LANG", "de_DE.UTF-8"), ("LC_CTYPE", "fr_FR.UTF-8")]).as_c_str(),
            c"fr_FR.UTF-8"
        );
        assert_eq!(
            from(&[("LC_ALL", ""), ("LC_CTYPE", "fr_FR.UTF-8")]).as_c_str(),
            c"fr_FR.UTF-8",
            "an empty variable is skipped"
        );
        assert_eq!(
            from(&[("LC_ALL", "pt_BR.UTF-8"), ("LC_CTYPE", "fr_FR.UTF-8")]).as_c_str(),
            c"pt_BR.UTF-8"
        );
    }
}
