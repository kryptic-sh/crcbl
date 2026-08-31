//! The atom table: every name this backend interns, and the cache it interns
//! them into.
//!
//! # Why atoms need a table at all
//!
//! X11 has no protocol constants for anything invented after 1987. Every EWMH
//! property, every selection, every mime type and every ICCCM protocol is a
//! *string* the client turns into a server-allocated integer with
//! `InternAtom`, and each of those is a round trip. A window manager
//! conversation touches around twenty of them, so interning on demand would put
//! twenty round trips on the path that creates a window.
//!
//! The fix is the one every X11 client uses and is worth stating because it is
//! the entire reason this module exists: **pipeline the requests, then collect
//! the replies.** libxcb splits every request into a cookie and a reply, so
//! [`Atoms::intern`] fires all twenty `InternAtom` requests before reading any
//! answer, and the whole table costs *one* round trip rather than twenty. Doing
//! it the other way is a startup that visibly stalls on a loaded server, and
//! it is the single most common performance mistake in hand-written X11 code.
//!
//! # What is in the table, and what is not
//!
//! Only names this backend actually sends or compares. `docs/plan/15-windowing.md`
//! scopes the X11 work at "what the shell actually uses", and an atom nobody
//! reads is a round trip nobody needed:
//!
//! | Group | Atoms | For |
//! | --- | --- | --- |
//! | ICCCM | `WM_PROTOCOLS`, `WM_DELETE_WINDOW` | the close request |
//! | EWMH | `_NET_WM_NAME`, `_NET_WM_STATE`, `_NET_WM_STATE_FULLSCREEN`, `_NET_SUPPORTING_WM_CHECK`, `_NET_WORKAREA`, `_NET_WM_PID` | the title, borderless, WM detection, the work area |
//! | Text | `UTF8_STRING` | `_NET_WM_NAME`'s type, and a clipboard target |
//! | Selection | `CLIPBOARD`, `TARGETS`, `INCR`, `CRCBL_SELECTION`, `CRCBL_TIMESTAMP` | the clipboard in both directions, and the timestamp a claim needs |
//! | XDND | `XdndAware`, `XdndEnter`, `XdndPosition`, `XdndStatus`, `XdndLeave`, `XdndDrop`, `XdndFinished`, `XdndSelection`, `XdndTypeList`, `XdndActionCopy`, `CRCBL_XDND` | the receiving half of drag and drop; see [`xdnd`](super::xdnd) |
//! | Mime | `text/plain;charset=utf-8`, `text/plain`, `application/x-crcbl+ron`, `text/uri-list` | the formats the seam names |
//!
//! Absent on purpose: `PRIMARY` (middle-click paste — the seam has no
//! vocabulary for a second selection, exactly as the Wayland backend says of
//! `wp_primary_selection_v1`), the XDND *source* atoms this client never sends
//! — `XdndActionMove`, `XdndActionLink`, `XdndActionAsk`, `XdndActionPrivate`,
//! `XdndActionList` and `XdndActionDescription`, all of which belong to
//! starting a drag or to negotiating an action other than copy — and
//! `_NET_WM_ICON` (an icon is pixels, and pixels are P1). `WM_STATE` is absent
//! too, and that one is a judgement rather than a deferral: it is how ICCCM
//! distinguishes *iconified* from *withdrawn*, and this backend does not need
//! the distinction — [`WindowState::visible`](crate::WindowState) is defined as
//! "mapped and not minimized", and a window manager minimizes a window by
//! unmapping it, so `MapNotify`/`UnmapNotify` already answer exactly that
//! question with no extra property and no extra round trip.

use core::ffi::c_char;

use crate::MimeType;

use super::ffi::{self, Connection, Lib};

/// One row of the table: the wire name, and which field of [`Atoms`] it fills.
///
/// A `const` array rather than a macro so the *order* is visible — the cookie
/// array and the field array are filled by the same index, and that
/// correspondence is the only thing keeping the pipelining above honest.
macro_rules! atom_table {
    ($($field:ident => $name:literal),+ $(,)?) => {
        /// Every atom this backend interns, resolved once at connection time.
        ///
        /// A plain struct of `u32`s. `0` is `XCB_ATOM_NONE` and means the
        /// server refused to intern the name, which cannot happen for a name
        /// this short but is handled as "the feature is off" rather than as a
        /// panic.
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
        #[allow(
            missing_docs,
            reason = "each field is named by the wire string it holds, which is \
                      in the table below and in the module docs; a doc comment \
                      per field would restate the literal"
        )]
        pub struct Atoms {
            $(pub $field: u32,)+
        }

        /// The wire names, in field order.
        const NAMES: &[&str] = &[$($name,)+];

        impl Atoms {
            /// Fills the struct from replies in the same order as [`NAMES`].
            fn from_values(values: &[u32]) -> Self {
                let mut index = 0;
                #[allow(unused_assignments, reason = "the last increment is dead by construction")]
                Self {
                    $($field: { let value = values.get(index).copied().unwrap_or(0); index += 1; value },)+
                }
            }
        }
    };
}

atom_table! {
    wm_protocols => "WM_PROTOCOLS",
    wm_delete_window => "WM_DELETE_WINDOW",
    net_wm_name => "_NET_WM_NAME",
    net_wm_state => "_NET_WM_STATE",
    net_wm_state_fullscreen => "_NET_WM_STATE_FULLSCREEN",
    net_supporting_wm_check => "_NET_SUPPORTING_WM_CHECK",
    net_workarea => "_NET_WORKAREA",
    net_wm_pid => "_NET_WM_PID",
    utf8_string => "UTF8_STRING",
    clipboard => "CLIPBOARD",
    targets => "TARGETS",
    timestamp => "TIMESTAMP",
    multiple => "MULTIPLE",
    atom_pair => "ATOM_PAIR",
    incr => "INCR",
    crcbl_selection => "CRCBL_SELECTION",
    crcbl_timestamp => "CRCBL_TIMESTAMP",
    xdnd_aware => "XdndAware",
    xdnd_enter => "XdndEnter",
    xdnd_position => "XdndPosition",
    xdnd_status => "XdndStatus",
    xdnd_leave => "XdndLeave",
    xdnd_drop => "XdndDrop",
    xdnd_finished => "XdndFinished",
    xdnd_selection => "XdndSelection",
    xdnd_type_list => "XdndTypeList",
    xdnd_action_copy => "XdndActionCopy",
    crcbl_xdnd => "CRCBL_XDND",
    mime_text_utf8 => "text/plain;charset=utf-8",
    mime_text_plain => "text/plain",
    mime_crcbl_ron => "application/x-crcbl+ron",
    mime_uri_list => "text/uri-list",
}

impl Atoms {
    /// Interns the whole table in one round trip.
    ///
    /// # Safety
    ///
    /// `connection` must be live and error-free.
    #[must_use]
    pub unsafe fn intern(lib: &'static Lib, connection: *mut Connection) -> Self {
        // Phase one: fire every request. Nothing is read yet, so libxcb writes
        // them all into one buffer and the server answers them back to back.
        let cookies: Vec<ffi::Cookie> = NAMES
            .iter()
            .map(|name| {
                let len = u16::try_from(name.len()).unwrap_or(u16::MAX);
                // SAFETY: the caller guarantees a live connection. `name` is a
                // `&str` from a literal in this file, valid for the call, and
                // `InternAtom` takes a counted string with no NUL requirement.
                // `only_if_exists = 0` creates the atom if it is new.
                unsafe { (lib.intern_atom)(connection, 0, len, name.as_ptr().cast::<c_char>()) }
            })
            .collect();

        // Phase two: collect. This is where the single round trip is paid.
        let values: Vec<u32> = cookies
            .into_iter()
            .map(|cookie| {
                // SAFETY: the cookie came from `intern_atom` on this
                // connection, and a null error pointer means "discard the
                // error", which is correct: a failed intern yields a null
                // reply, which is already handled below.
                let reply =
                    unsafe { (lib.intern_atom_reply)(connection, cookie, core::ptr::null_mut()) };
                if reply.is_null() {
                    return 0;
                }
                // SAFETY: `reply` is a live `xcb_intern_atom_reply_t` this call
                // owns; it is read once and freed immediately.
                let atom = unsafe { (*reply).atom };
                // SAFETY: `reply` came from libxcb's `malloc` and is freed once.
                unsafe { ffi::free_reply(reply) };
                atom
            })
            .collect();

        Self::from_values(&values)
    }

    /// The atom naming `mime`, or `0` for a format this table does not carry.
    ///
    /// The X11 spelling of the seam's [`MimeType`]. `Other` is deliberately
    /// unmapped rather than interned on demand: interning inside a selection
    /// handler would put a round trip in the middle of answering a
    /// `SelectionRequest`, which is precisely where a round trip deadlocks
    /// against a peer waiting on us.
    #[must_use]
    pub fn for_mime(&self, mime: MimeType) -> u32 {
        match mime {
            MimeType::TextUtf8 => self.mime_text_utf8,
            MimeType::CrcblRon => self.mime_crcbl_ron,
            MimeType::UriList => self.mime_uri_list,
            MimeType::Other(_) => 0,
        }
    }

    /// Every atom that means "UTF-8 text", in the order they should be offered.
    ///
    /// X11 clients ask for text under at least four names and mean the same
    /// thing by all of them. [`MimeType::recognize`] already accepts the
    /// strings; this is the atom-level counterpart, and the *order* matters
    /// because it is the order `TARGETS` advertises: a peer that takes the
    /// first thing it recognizes should get UTF-8 rather than Latin-1
    /// `STRING`.
    #[must_use]
    pub fn text_targets(&self) -> [u32; 4] {
        [
            self.mime_text_utf8,
            self.utf8_string,
            self.mime_text_plain,
            ffi::value::ATOM_STRING,
        ]
    }

    /// Every atom `mime` may be spelled as on the wire, best first.
    ///
    /// The list [`ConvertSelection`'s `TARGETS` step](super::selection) picks
    /// from. Text is the interesting one: four spellings, all meaning UTF-8
    /// except the last, and which of them a peer offers depends entirely on
    /// what toolkit it was written with. Asking for one and giving up is how a
    /// clipboard ends up working with GTK applications and not with `xterm`.
    #[must_use]
    pub fn candidates_for(&self, mime: MimeType) -> Vec<u32> {
        let mut candidates = match mime {
            MimeType::TextUtf8 => self.text_targets().to_vec(),
            other => vec![self.for_mime(other)],
        };
        candidates.retain(|atom| *atom != 0);
        candidates
    }

    /// The [`MimeType`] a target atom names, if this backend knows the format.
    ///
    /// The inverse of [`for_mime`](Self::for_mime), and deliberately more
    /// permissive: an incoming request may name any of the text spellings.
    #[must_use]
    pub fn mime_for_target(&self, target: u32) -> Option<MimeType> {
        if target == 0 {
            return None;
        }
        if self.text_targets().contains(&target) {
            return Some(MimeType::TextUtf8);
        }
        if target == self.mime_crcbl_ron {
            return Some(MimeType::CrcblRon);
        }
        if target == self.mime_uri_list {
            return Some(MimeType::UriList);
        }
        None
    }

    /// How the *peer* spelled a target, for
    /// [`ShellEvent::ClipboardData::mime`](crate::ShellEvent::ClipboardData).
    ///
    /// [`ReceivedMime`](crate::ReceivedMime) exists because the peer chose the
    /// string, and on X11 the peer chose an *atom*; this maps the ones we
    /// offered back to the exact string that atom stands for, so a
    /// `UTF8_STRING` answer reports `"UTF8_STRING"` and not
    /// `"text/plain;charset=utf-8"`. An atom we did not offer has no known
    /// spelling and answers `None` — the caller then falls back to the format
    /// it asked for, which is what the seam specifies.
    #[must_use]
    pub fn spelling_of(&self, target: u32) -> Option<&'static str> {
        let known = [
            (self.mime_text_utf8, MimeType::TextUtf8.as_str()),
            (self.utf8_string, "UTF8_STRING"),
            (self.mime_text_plain, "text/plain"),
            (ffi::value::ATOM_STRING, "STRING"),
            (self.mime_crcbl_ron, MimeType::CrcblRon.as_str()),
            (self.mime_uri_list, MimeType::UriList.as_str()),
        ];
        known
            .into_iter()
            .find(|(atom, _)| *atom != 0 && *atom == target)
            .map(|(_, name)| name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The last atom the protocol itself defines, `XA_LAST_PREDEFINED`.
    ///
    /// A server allocates above it and can never allocate below it, which is
    /// what makes the table below a model of a real server rather than merely a
    /// set of distinct integers. Numbering from 1 put `application/x-crcbl+ron`
    /// on 31 — which is permanently `STRING`, one of [`Atoms::text_targets`] —
    /// so `mime_for_target` answered `TextUtf8` for it and the collision was
    /// the fixture's, not the code's.
    const LAST_PREDEFINED: u32 = 68;

    /// A table with distinct, non-zero values, as a live server would produce.
    fn table() -> Atoms {
        let values: Vec<u32> = (1..=u32::try_from(NAMES.len()).expect("small"))
            .map(|index| LAST_PREDEFINED + index)
            .collect();
        Atoms::from_values(&values)
    }

    #[test]
    fn the_table_is_filled_in_name_order() {
        // The invariant the whole pipelining scheme rests on: reply `i`
        // belongs to name `i`. Getting this wrong makes `WM_PROTOCOLS` hold
        // `WM_DELETE_WINDOW`'s atom, which fails in a way that looks like a
        // window manager bug.
        let atoms = table();
        assert_eq!(atoms.wm_protocols, LAST_PREDEFINED + 1, "the first name");
        assert_eq!(atoms.wm_delete_window, LAST_PREDEFINED + 2);
        assert_eq!(
            atoms.mime_uri_list,
            LAST_PREDEFINED + u32::try_from(NAMES.len()).expect("small"),
            "the last name"
        );

        let mut seen: Vec<u32> = vec![
            atoms.wm_protocols,
            atoms.wm_delete_window,
            atoms.net_wm_name,
            atoms.net_wm_state,
            atoms.net_wm_state_fullscreen,
            atoms.net_supporting_wm_check,
            atoms.net_workarea,
            atoms.net_wm_pid,
            atoms.utf8_string,
            atoms.clipboard,
            atoms.targets,
            atoms.timestamp,
            atoms.multiple,
            atoms.atom_pair,
            atoms.incr,
            atoms.crcbl_selection,
            atoms.crcbl_timestamp,
            atoms.xdnd_aware,
            atoms.xdnd_enter,
            atoms.xdnd_position,
            atoms.xdnd_status,
            atoms.xdnd_leave,
            atoms.xdnd_drop,
            atoms.xdnd_finished,
            atoms.xdnd_selection,
            atoms.xdnd_type_list,
            atoms.xdnd_action_copy,
            atoms.crcbl_xdnd,
            atoms.mime_text_utf8,
            atoms.mime_text_plain,
            atoms.mime_crcbl_ron,
            atoms.mime_uri_list,
        ];
        assert_eq!(seen.len(), NAMES.len(), "every name has a field");
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), NAMES.len(), "no field is filled twice");
    }

    #[test]
    fn a_server_that_refuses_an_intern_leaves_a_zero_rather_than_shifting() {
        // A short reply list must not slide every later atom up by one — that
        // would be silent corruption of the whole table.
        let atoms = Atoms::from_values(&[7, 9]);
        assert_eq!(atoms.wm_protocols, 7);
        assert_eq!(atoms.wm_delete_window, 9);
        assert_eq!(atoms.net_wm_name, 0);
        assert_eq!(atoms.mime_uri_list, 0);
        assert_eq!(Atoms::default(), Atoms::from_values(&[]));
    }

    #[test]
    fn the_engine_mime_types_map_to_atoms_and_back() {
        let atoms = table();
        for mime in [MimeType::TextUtf8, MimeType::CrcblRon, MimeType::UriList] {
            let target = atoms.for_mime(mime);
            assert_ne!(target, 0, "{mime}");
            assert_eq!(atoms.mime_for_target(target), Some(mime), "{mime}");
            assert_eq!(atoms.spelling_of(target), Some(mime.as_str()), "{mime}");
        }
        // `Other` is not interned, by decision — see `for_mime`.
        assert_eq!(atoms.for_mime(MimeType::Other("image/png")), 0);
        assert_eq!(atoms.mime_for_target(0), None, "ATOM_NONE is not a format");
    }

    #[test]
    fn a_read_asks_for_every_spelling_of_text_and_one_of_everything_else() {
        let atoms = table();
        assert_eq!(
            atoms.candidates_for(MimeType::TextUtf8),
            atoms.text_targets().to_vec(),
            "four spellings, in preference order"
        );
        assert_eq!(
            atoms.candidates_for(MimeType::CrcblRon),
            vec![atoms.mime_crcbl_ron],
            "the engine's own format has exactly one spelling"
        );
        assert!(
            atoms
                .candidates_for(MimeType::Other("image/png"))
                .is_empty(),
            "a format with no atom cannot be asked for"
        );
    }

    #[test]
    fn every_text_spelling_an_x11_peer_uses_is_accepted() {
        // The clipboard iceberg's first layer: four atoms, one meaning. A
        // backend that only recognized `text/plain;charset=utf-8` would refuse
        // to paste from xterm, which offers `STRING` and `UTF8_STRING` only.
        let atoms = table();
        for target in atoms.text_targets() {
            assert_eq!(
                atoms.mime_for_target(target),
                Some(MimeType::TextUtf8),
                "atom {target}"
            );
        }
        assert_eq!(
            atoms.text_targets()[0],
            atoms.mime_text_utf8,
            "the most specific spelling is advertised first"
        );
        assert_eq!(
            atoms.text_targets()[3],
            ffi::value::ATOM_STRING,
            "Latin-1 STRING is the last resort, and is a predefined atom"
        );
        // The peer's spelling survives verbatim, which is what `ReceivedMime`
        // is for.
        assert_eq!(atoms.spelling_of(atoms.utf8_string), Some("UTF8_STRING"));
        assert_eq!(
            atoms.spelling_of(ffi::value::ATOM_STRING),
            Some("STRING"),
            "a predefined atom still has a spelling"
        );
        assert_eq!(atoms.spelling_of(999_999), None, "an atom we never offered");
    }
}
