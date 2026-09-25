//! The Win32 clipboard: which format a mime type is, how a payload is encoded,
//! and the guard that makes "closed on every path out" structural.
//!
//! The arithmetic and the decisions are pure and compile on every host — the
//! format mapping, the UTF-16 conversion in both directions, the retry budget —
//! so the parts that can be got wrong quietly are exercised by `cargo test` on
//! the machine this was written on. Everything that calls `user32` is
//! [`Clipboard`], and that is Windows-only.
//!
//! # Win32 hands the bytes over; it does not lend them
//!
//! Every other backend's clipboard write is a *claim* that has to be serviced
//! later: X11 records an offer and answers `SelectionRequest`s from it, Wayland
//! keeps a `wl_data_source` and writes into a pipe when the compositor asks.
//! Both hold the payload for as long as they own the selection, and both have a
//! whole state machine for the transfer.
//!
//! `SetClipboardData` **takes the memory**. Ownership of the `HGLOBAL` passes to
//! the system, the bytes live in the window station rather than in this process,
//! and there is no later conversation to service — which is why this module has
//! no equivalent of `x11::selection` and no timeout on the write side. The
//! shell keeps nothing.
//!
//! Windows does have the lending shape: `SetClipboardData` with a **null**
//! handle claims a format and promises to produce it from `WM_RENDERFORMAT`.
//! **This backend cannot use it**, and the reason is the one that shaped
//! everything else here: `WM_RENDERFORMAT` has to be answered *synchronously,
//! from the window procedure*, and this backend's procedure records rather than
//! acts because it runs re-entrantly inside the shell's own calls (see
//! [`proc`](super::proc)). An owner that defers the answer to the next
//! [`pump`](crate::Shell::pump) has already returned an empty format to whoever
//! asked. Rendering immediately costs a copy of the payload and owes nobody
//! anything afterwards, including at process exit, where a delayed-render owner
//! must answer `WM_RENDERALLFORMATS` before it may quit.
//!
//! # There is no `TARGETS` negotiation, because the system does it
//!
//! X11 spends a whole round trip asking the owner which spellings of "text" it
//! has, because `STRING`, `UTF8_STRING` and `text/plain;charset=utf-8` are three
//! atoms and an owner answers only what it was asked for. Windows has one text
//! format worth writing — [`CF_UNICODETEXT`](super::ffi::value::CF_UNICODETEXT)
//! — and **synthesizes** `CF_TEXT` and `CF_OEMTEXT` from it for applications
//! that want those, and synthesizes it back from them for us. So an ANSI
//! application pastes what this engine copied, this engine pastes what an ANSI
//! application copied, and neither direction needs a negotiation step.
//!
//! Everything that is not text goes under a **registered** format, which is a
//! format named by an arbitrary string: `RegisterClipboardFormatW` interns the
//! name in the window station and answers the same id in every process that
//! asks for it. That is exactly a mime type, so [`MimeType::CrcblRon`] is
//! published under `application/x-crcbl+ron` and read back losslessly, while
//! [`MimeType::TextUtf8`] reaches Notepad. Offering both is the caller's job
//! and the reader picks, which is what the windowing rules in
//! `docs/notes/backends.md` specify.
//!
//! # `text/uri-list` is also read from `CF_HDROP`
//!
//! A "copy" of files in Explorer publishes `CF_HDROP` — the same file-list
//! block a drop hands over — and no `text/uri-list`. So a
//! [`MimeType::UriList`] request that finds no registered format of that name
//! reads `CF_HDROP` instead, through `DragQueryFileW` exactly as
//! [`dnd`](super::dnd) reads a drop, and answers an RFC 2483 list: one
//! `file:` URI per path, each line CRLF-terminated, encoded with
//! `crate::clipboard::windows_uri` — the module
//! [`parse_uri_list`](crate::parse_uri_list) decodes with on Windows, so the
//! paths come back as they were.
//!
//! **The registered format wins when both are there.**
//! Its bytes are exactly what a uri-list-aware publisher wrote, where the
//! `CF_HDROP` rendering is a synthesis; it can carry what a file list cannot
//! (a non-`file:` URI, a comment line); and it keeps every request that was
//! answered before `CF_HDROP` was read answered byte for byte as it was.
//! There is no staleness to weigh: every write starts with `EmptyClipboard`,
//! so two formats on the clipboard at once were put there by one publisher.
//!
//! # A `text/uri-list` offer is also published as `CF_HDROP`
//!
//! The write direction of the same bridge: Explorer pastes files from
//! `CF_HDROP` and knows nothing of a registered `text/uri-list`, so an offer
//! of one publishes both. The registered format carries the caller's bytes
//! verbatim, as every other registered offer does; the `CF_HDROP` beside it is
//! a [`drop_files_block`] naming the paths [`hdrop_paths`] decodes from those
//! bytes, with `crate::clipboard::windows_uri` — the decoder
//! [`parse_uri_list`](crate::parse_uri_list) uses on Windows, so a `CF_HDROP`
//! reader and a uri-list reader see the same files.
//!
//! **A URI that names no Windows file is left out of the file list, logged.**
//! A file list holds paths and nothing else, so an `https:` URI, a POSIX
//! `file:///tmp/x` with no drive to resolve against, or a name that decodes
//! to a separator has nowhere to go; the registered format still carries it.
//! One unnameable entry does not cost the rest of the list — the rule the read
//! direction follows. When **no** entry is nameable, no `CF_HDROP` is
//! published at all: an empty file list is a paste of nothing, which is worse
//! than Explorer greying out Paste.
//!
//! Beside the file list goes a registered `Preferred DropEffect` of
//! `DROPEFFECT_COPY` ([`PREFERRED_DROP_EFFECT`], [`DROP_EFFECT_COPY`]). Explorer
//! reads it on paste to choose between copying the files and moving them —
//! "cut" in Explorer publishes `DROPEFFECT_MOVE` there — and a clipboard offer
//! from the engine is never a request to delete the originals. Without it the
//! choice is left to whichever application pastes, which is not ours to leave
//! to chance.
//!
//! Both are rendered immediately, like every other format here, for the
//! reason the first section gives.
//!
//! # Decision: a payload is NUL-terminated and read back NUL-trimmed
//!
//! `GlobalSize` is documented to answer **at least** what was asked for: a block
//! may be larger than the request, and the extra bytes would otherwise be
//! whatever the heap left there. Two things together make that a non-problem
//! rather than a documented hazard:
//!
//! * [`Clipboard::put`] zeroes the **whole** allocation before copying, so the
//!   padding is NUL bytes rather than heap contents. Nothing uninitialised
//!   reaches another process, which is a soundness question as much as a
//!   correctness one.
//! * A payload is written with a terminator — the UTF-16 NUL `CF_UNICODETEXT`
//!   is defined to carry, one NUL byte for a registered format — and read back
//!   through [`utf8_from_utf16_bytes`] or [`trim_trailing_nuls`], which stop at
//!   it. This is also what other applications do for registered text formats,
//!   so it is compatibility rather than a private convention.
//!
//! The edge, named here rather than discovered: a payload whose own last bytes
//! are NUL loses them. Neither of the engine's two formats is binary, and the
//! alternative — handing back the heap's padding — is worse in every case.
//!
//! # Decision: `OpenClipboard` is retried, with a bound
//!
//! **`OpenClipboard` fails while another process has the clipboard open**, and
//! that is routine: Explorer, a clipboard manager and every other application
//! open it for the microseconds around their own reads. A backend that answered
//! [`Unavailable`](crate::ClipboardContent::Unavailable) on the first refusal
//! would fail a paste for a reason that was over before the user noticed.
//!
//! So it is retried [`OPEN_ATTEMPTS`] times, [`OPEN_RETRY`] apart, and then
//! answered `Unavailable`. That is [obligation 4](crate::Shell) satisfied rather
//! than dodged: the bound is [`OPEN_BUDGET`], it is part of the
//! [`clipboard_request`](crate::Shell::clipboard_request) call rather than a
//! deadline carried across pumps, and there is no path on which a request is
//! accepted and never answered. Unbounded retrying would be the violation, and
//! so would giving up instantly — the first hangs the thread on another
//! process's behaviour, the second reports a failure that is not one.
//!
//! [`Opened`] is what a log line and a test read, for the reason
//! [`Wake`](super::shell) exists one file over: a retry that succeeded and an
//! open that never had to retry take indistinguishable amounts of time on a
//! loaded machine, and only the code knows which happened.

use core::time::Duration;

use crate::MimeType;

/// How a payload is carried, once the mime type has been decided.
///
/// The whole of the format decision, and pure so that it can be checked without
/// a window station: getting it wrong means a copy that no other application
/// can read, which is invisible until somebody tries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    /// `CF_UNICODETEXT`: UTF-16, NUL-terminated, and understood by everything.
    UnicodeText,
    /// A format registered under this name, carrying the bytes verbatim.
    Registered(&'static str),
}

/// Which format a mime type is published and read under.
///
/// Only [`TextUtf8`](MimeType::TextUtf8) is a built-in Windows format. Every
/// other mime — the engine's RON, `text/uri-list`, and anything an
/// [`Other`](MimeType::Other) names — is registered under its own mime string,
/// which is what a browser and every other application exchanging custom types
/// on Windows already does. A `text/uri-list` read that finds none falls back
/// to `CF_HDROP`; see the [module docs](self).
#[must_use]
pub const fn encoding_for(mime: MimeType) -> Encoding {
    match mime {
        MimeType::TextUtf8 => Encoding::UnicodeText,
        MimeType::CrcblRon | MimeType::UriList | MimeType::Other(_) => {
            Encoding::Registered(mime.as_str())
        }
    }
}

/// A payload as the bytes that go on the clipboard for `encoding`.
///
/// The terminator is added here rather than by the caller, so that the write
/// side and the two read sides below cannot disagree about whether there is one.
///
/// Text is decoded **lossily** when the caller's bytes are not valid UTF-8.
/// [`ClipboardOffer`](crate::ClipboardOffer) documents that it does not validate
/// — "validation belongs to whoever built it" — and `CF_UNICODETEXT` has no
/// representation for a byte that is not a character. The caller logs when it
/// happens; there is nothing else to do that is not worse than a replacement
/// character.
#[must_use]
pub fn payload_bytes(encoding: Encoding, bytes: &[u8]) -> Vec<u8> {
    match encoding {
        Encoding::UnicodeText => utf16_nul_bytes(&String::from_utf8_lossy(bytes)),
        Encoding::Registered(_) => {
            let mut payload = Vec::with_capacity(bytes.len() + 1);
            payload.extend_from_slice(bytes);
            payload.push(0);
            payload
        }
    }
}

/// Text as the NUL-terminated UTF-16 `CF_UNICODETEXT` is defined as, in bytes.
///
/// Native byte order, which is what an in-process buffer handed to a
/// same-machine API means and what every Windows target this backend has is.
#[must_use]
pub fn utf16_nul_bytes(text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(text.len() * 2 + 2);
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_ne_bytes());
    }
    bytes.extend_from_slice(&0u16.to_ne_bytes());
    bytes
}

/// The reverse: a `CF_UNICODETEXT` block as UTF-8, stopping at the terminator.
///
/// Three things this has to survive, and all three are ordinary rather than
/// hostile:
///
/// * **Padding after the terminator**, from a `GlobalSize` larger than the
///   write. Stopping at the first NUL unit is what makes that invisible.
/// * **No terminator at all**, from an application that wrote the length
///   exactly. Then the whole block is the string.
/// * **An unpaired surrogate**, which UTF-16 permits and UTF-8 cannot represent
///   at all. It becomes U+FFFD, because the alternative is failing a paste over
///   one code unit in the middle of a document.
///
/// An odd trailing byte — a block whose size is not a multiple of two — is not
/// half of a code unit anyone can use, and is dropped.
#[must_use]
pub fn utf8_from_utf16_bytes(bytes: &[u8]) -> Vec<u8> {
    let mut units = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.as_chunks::<2>().0 {
        let unit = u16::from_ne_bytes([pair[0], pair[1]]);
        if unit == 0 {
            break;
        }
        units.push(unit);
    }
    String::from_utf16_lossy(&units).into_bytes()
}

/// A registered format's payload, without the terminator or any padding.
///
/// See the [module docs](self) for why trailing NULs are trimmed rather than
/// exactly one being removed, and for the one payload shape that costs.
#[must_use]
pub fn trim_trailing_nuls(bytes: &[u8]) -> &[u8] {
    let mut end = bytes.len();
    while end > 0 && bytes[end - 1] == 0 {
        end -= 1;
    }
    &bytes[..end]
}

/// `sizeof(DROPFILES)`, which is also the `pFiles` offset of a file list that
/// starts straight after its header.
///
/// `shlobj_core.h` declares `DROPFILES` under `pshpack1.h`: a `DWORD`, a
/// `POINT` and two `BOOL`s with no padding between them.
pub const DROP_FILES_HEADER: u32 = 20;

/// The registered format Explorer reads on paste to choose between copying
/// the files a `CF_HDROP` names and moving them.
#[cfg(target_os = "windows")]
pub const PREFERRED_DROP_EFFECT: &str = "Preferred DropEffect";

/// `DROPEFFECT_COPY`, from `oleidl.h` — the `DWORD` a
/// [`PREFERRED_DROP_EFFECT`] carries when the originals are to be kept.
#[cfg(target_os = "windows")]
pub const DROP_EFFECT_COPY: u32 = 1;

/// The `CF_HDROP` block naming `paths`: a `DROPFILES` header followed by a
/// wide file list.
///
/// Built field by field rather than by writing a `#[repr(C)]` structure,
/// because the SDK's structure is packed and a Rust one of the same fields is
/// not; spelling the five fields out at their offsets leaves no padding rule
/// to disagree about. Every field is little-endian, which is what every
/// Windows target is.
///
/// * `pFiles` is [`DROP_FILES_HEADER`]: the list starts after the header.
/// * `pt` and `fNC` are zero — they describe where a *drop* landed, and a
///   clipboard file list landed nowhere.
/// * `fWide` is `TRUE`, because the list is UTF-16.
///
/// The list is each path followed by a NUL, then one more NUL that ends it.
#[must_use]
pub fn drop_files_block<'a>(paths: impl IntoIterator<Item = &'a str>) -> Vec<u8> {
    let mut block = Vec::new();
    // pFiles, pt.x, pt.y, fNC, fWide.
    for field in [DROP_FILES_HEADER, 0, 0, 0, 1] {
        block.extend_from_slice(&field.to_le_bytes());
    }
    for path in paths {
        for unit in path.encode_utf16() {
            block.extend_from_slice(&unit.to_le_bytes());
        }
        block.extend_from_slice(&0u16.to_le_bytes());
    }
    block.extend_from_slice(&0u16.to_le_bytes());
    block
}

/// The Windows paths a `text/uri-list` names, for its `CF_HDROP` rendering.
///
/// Comment and blank lines are skipped as [`parse_uri_list`](crate::parse_uri_list)
/// skips them. A URI that names no Windows file — see the
/// [module docs](self) — is left out and logged.
#[must_use]
pub fn hdrop_paths(uri_list: &[u8]) -> Vec<String> {
    crate::clipboard::uri_list_lines(uri_list)
        .filter_map(|uri| {
            let path = crate::clipboard::windows_uri::file_uri_to_windows_path(uri);
            if path.is_none() {
                crcbl_core::log::warn!(
                    "{:?} names no Windows file, so it is left out of the CF_HDROP file list",
                    String::from_utf8_lossy(uri)
                );
            }
            path
        })
        .collect()
}

/// How many times `OpenClipboard` is tried before the read or write gives up.
pub const OPEN_ATTEMPTS: u32 = 8;

/// How long to wait between attempts.
///
/// Long enough that the attempts span the moment another process holds the
/// clipboard for, short enough that the whole budget is a fraction of a frame's
/// worth of stall on the thread that pumps.
pub const OPEN_RETRY: Duration = Duration::from_millis(10);

/// The whole of the wait an [`OpenClipboard`](Clipboard::open) can cost.
///
/// The bound [obligation 4](crate::Shell) asks for, as a value rather than as a
/// sentence — the last attempt is not followed by a sleep.
pub const OPEN_BUDGET: Duration = match OPEN_RETRY.checked_mul(OPEN_ATTEMPTS - 1) {
    Some(budget) => budget,
    None => Duration::MAX,
};

#[cfg(target_os = "windows")]
pub(super) use system::{Clipboard, Opened, format_id};

/// The half that calls `user32` and `kernel32`.
#[cfg(target_os = "windows")]
mod system {
    use core::ptr;

    use super::{Encoding, OPEN_ATTEMPTS, OPEN_RETRY, encoding_for};
    use crate::MimeType;

    use super::super::ffi::{self, Handle, value};

    /// How an [`Clipboard::open`] ended.
    ///
    /// Named rather than reduced to a boolean for the reason
    /// [`Wake`](super::super::shell) is: the three outcomes take
    /// indistinguishable amounts of time on a loaded machine, so a wall clock
    /// cannot tell an open that never had to retry from one that retried seven
    /// times and from one that failed. This can, and it is what a log line and a
    /// test assert on.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(in super::super) enum Opened {
        /// The first attempt took it, which is the ordinary case.
        Immediately,
        /// Another process had it and let go. Carries the attempt that worked.
        After {
            /// Which attempt succeeded, counting from one.
            attempts: u32,
        },
        /// Every attempt was refused; the clipboard belongs to somebody who is
        /// not letting go.
        Refused {
            /// How many were made.
            attempts: u32,
            /// `GetLastError` after the last one.
            error: u32,
        },
    }

    /// The clipboard, open, and **closed when this is dropped**.
    ///
    /// A guard rather than an open/close pair because the requirement is
    /// "closed on every path out, including the error paths", and a pair makes
    /// that a discipline that every `?` and every early return has to remember.
    /// Leaving the clipboard open is not a leak that shows up later: it locks
    /// every other application on the desktop out of the clipboard until this
    /// process exits.
    pub(in super::super) struct Clipboard {
        opened: Opened,
    }

    impl Clipboard {
        /// Opens the clipboard for `hwnd`, retrying a bounded number of times.
        ///
        /// See the [module docs](super) for why a refusal is retried at all and
        /// why the retry is bounded.
        ///
        /// # Errors
        ///
        /// [`Opened::Refused`], carrying the attempt count and the last
        /// `GetLastError`.
        pub(in super::super) fn open(hwnd: Handle) -> Result<Self, Opened> {
            let mut error = 0;
            for attempt in 1..=OPEN_ATTEMPTS {
                // SAFETY: `hwnd` is a live window of this shell, on the thread
                // that owns it. The association is with the window, and the
                // clipboard is released by this type's `Drop`.
                if unsafe { ffi::OpenClipboard(hwnd) } != 0 {
                    let opened = if attempt == 1 {
                        Opened::Immediately
                    } else {
                        Opened::After { attempts: attempt }
                    };
                    return Ok(Self { opened });
                }
                // SAFETY: reads this thread's last error code, which the failed
                // call above set.
                error = unsafe { ffi::GetLastError() };
                if attempt < OPEN_ATTEMPTS {
                    std::thread::sleep(OPEN_RETRY);
                }
            }
            Err(Opened::Refused {
                attempts: OPEN_ATTEMPTS,
                error,
            })
        }

        /// How the open went.
        pub(in super::super) const fn opened(&self) -> Opened {
            self.opened
        }

        /// Discards everything on the clipboard and makes this process its
        /// owner.
        ///
        /// Required before `SetClipboardData` — the system refuses a write from
        /// a process that has not emptied it — and it is also the whole of what
        /// "release the clipboard" can mean here; see
        /// [`clipboard_offer`](crate::Shell::clipboard_offer) on this backend.
        pub(in super::super) fn empty(&self) -> bool {
            // SAFETY: this thread has the clipboard open, which is the call's
            // only precondition.
            unsafe { ffi::EmptyClipboard() != 0 }
        }

        /// Publishes `bytes` under `format`.
        ///
        /// The allocation is **zeroed in full** before the copy, so that a
        /// `GlobalSize` larger than the request cannot hand another process
        /// uninitialised heap — see the [module docs](super).
        pub(in super::super) fn put(&self, format: u32, bytes: &[u8]) -> bool {
            let requested = bytes.len().max(1);
            // SAFETY: an allocation request by value. `GMEM_MOVEABLE` is what
            // `SetClipboardData` requires; the handle is freed below on every
            // path where ownership does not pass to the system.
            let mem = unsafe { ffi::GlobalAlloc(value::GMEM_MOVEABLE, requested) };
            if mem.is_null() {
                crcbl_core::log::warn!("GlobalAlloc of {requested} bytes for the clipboard failed");
                return false;
            }
            // SAFETY: `mem` is a live `GMEM_MOVEABLE` handle this call owns.
            let allocated = unsafe { ffi::GlobalSize(mem) }.max(requested);
            // SAFETY: as above; a moveable block has no address until it is
            // locked, and the lock is released below.
            let locked = unsafe { ffi::GlobalLock(mem) };
            if locked.is_null() {
                // SAFETY: freeing a block this call allocated and did not hand
                // over.
                unsafe { ffi::GlobalFree(mem) };
                crcbl_core::log::warn!("GlobalLock of a clipboard payload failed");
                return false;
            }
            // SAFETY: `locked` points at `allocated` writable bytes — the size
            // the system just reported for this block — and `bytes` is a slice
            // of `bytes.len() <= allocated` bytes that does not overlap it,
            // because it was allocated a moment ago.
            unsafe {
                ptr::write_bytes(locked.cast::<u8>(), 0, allocated);
                ptr::copy_nonoverlapping(bytes.as_ptr(), locked.cast::<u8>(), bytes.len());
                ffi::GlobalUnlock(mem);
            }
            // SAFETY: this thread has the clipboard open and has emptied it,
            // and `mem` is a `GMEM_MOVEABLE` block. **Ownership passes to the
            // system on success** and stays here on failure, which is the
            // asymmetry the free below covers.
            if unsafe { ffi::SetClipboardData(format, mem) }.is_null() {
                // SAFETY: ownership did not pass, so this block is still ours.
                let error = unsafe {
                    ffi::GlobalFree(mem);
                    ffi::GetLastError()
                };
                crcbl_core::log::warn!(
                    "SetClipboardData for format {format} failed with Win32 error {error}"
                );
                return false;
            }
            true
        }

        /// The bytes on the clipboard under `format`, or `None` when it holds
        /// nothing in it.
        ///
        /// The handle belongs to the clipboard: it is neither freed nor kept,
        /// and the copy is taken while this guard still holds the clipboard
        /// open, because the handle is not valid after it is closed.
        pub(in super::super) fn get(&self, format: u32) -> Option<Vec<u8>> {
            // SAFETY: this thread has the clipboard open. The returned handle
            // is owned by the clipboard and must not be freed.
            let mem = unsafe { ffi::GetClipboardData(format) };
            if mem.is_null() {
                return None;
            }
            // SAFETY: `mem` is a live handle the clipboard owns.
            let size = unsafe { ffi::GlobalSize(mem) };
            if size == 0 {
                // A format that is present and empty. `GlobalLock` on a
                // zero-length block answers null, so there is nothing to read
                // and nothing has gone wrong.
                return Some(Vec::new());
            }
            // SAFETY: as above; the lock is released below.
            let locked = unsafe { ffi::GlobalLock(mem) };
            if locked.is_null() {
                crcbl_core::log::warn!("GlobalLock of clipboard format {format} failed");
                return None;
            }
            // SAFETY: `locked` points at `size` readable bytes — the size the
            // system just reported for this block — and stays valid for the
            // length of the copy because the lock is still held.
            let bytes = unsafe { core::slice::from_raw_parts(locked.cast::<u8>(), size) }.to_vec();
            // SAFETY: balancing the lock taken above, on the same handle.
            unsafe { ffi::GlobalUnlock(mem) };
            Some(bytes)
        }

        /// The files a `CF_HDROP` on the clipboard names, as a
        /// `text/uri-list`, or `None` when the clipboard holds no `CF_HDROP`.
        ///
        /// See the [module docs](super) for when this is asked. A path that
        /// has no URI — one that is not valid Unicode, or not absolute — is
        /// left out and logged, rather than failing the whole list.
        pub(in super::super) fn file_uri_list(&self) -> Option<Vec<u8>> {
            // SAFETY: this thread has the clipboard open. The returned handle
            // is owned by the clipboard and must not be freed or finished.
            let hdrop = unsafe { ffi::GetClipboardData(value::CF_HDROP) };
            if hdrop.is_null() {
                return None;
            }
            // SAFETY: a `CF_HDROP` block is an `HDROP`, and it stays live while
            // this guard holds the clipboard open, which it does for the whole
            // call. `query_files` neither finishes nor frees it.
            let paths = unsafe { super::super::dnd::query_files(hdrop) };
            let names = paths.iter().filter_map(|path| {
                let name = path.to_str();
                if name.is_none() {
                    crcbl_core::log::warn!(
                        "{path:?} is not valid Unicode, so it has no text/uri-list entry"
                    );
                }
                name
            });
            Some(crate::clipboard::windows_uri::uri_list_from_windows_paths(names).into_bytes())
        }

        /// Publishes the files a `text/uri-list` names as a `CF_HDROP`, with a
        /// `Preferred DropEffect` of copy beside it, and answers whether the
        /// `CF_HDROP` was published.
        ///
        /// Nothing is published when no URI in the list names a Windows file;
        /// see the [module docs](super). A `Preferred DropEffect` that cannot
        /// be published is logged and does not fail the file list, which
        /// Explorer then pastes under its own default.
        pub(in super::super) fn put_file_list(&self, uri_list: &[u8]) -> bool {
            let paths = super::hdrop_paths(uri_list);
            if paths.is_empty() {
                crcbl_core::log::debug!(
                    "the text/uri-list names no Windows file, so no CF_HDROP is published"
                );
                return false;
            }
            let block = super::drop_files_block(paths.iter().map(String::as_str));
            if !self.put(value::CF_HDROP, &block) {
                return false;
            }
            let effect = registered_format(super::PREFERRED_DROP_EFFECT)
                .is_some_and(|format| self.put(format, &super::DROP_EFFECT_COPY.to_le_bytes()));
            if !effect {
                crcbl_core::log::warn!(
                    "the CF_HDROP was published without a Preferred DropEffect; Explorer will \
                     choose between copy and move by itself"
                );
            }
            true
        }
    }

    impl Drop for Clipboard {
        /// Closes the clipboard.
        ///
        /// The whole reason this type exists: there is no path out of a
        /// function holding one that does not run this.
        fn drop(&mut self) {
            // SAFETY: this thread opened the clipboard in `open` and has not
            // closed it; the call takes no arguments.
            unsafe { ffi::CloseClipboard() };
        }
    }

    /// The clipboard format number a mime type is carried under.
    ///
    /// `None` only when the mime string cannot be a format name — an interior
    /// NUL — or when the window station refused to register it, which is a
    /// full atom table and has never been seen.
    pub(in super::super) fn format_id(mime: MimeType) -> Option<u32> {
        match encoding_for(mime) {
            Encoding::UnicodeText => Some(value::CF_UNICODETEXT),
            Encoding::Registered(name) => registered_format(name),
        }
    }

    /// The format number the window station interns `name` under, or `None`
    /// for the two reasons [`format_id`] gives.
    fn registered_format(name: &str) -> Option<u32> {
        let wide = ffi::wide(name)?;
        // SAFETY: `wide` is NUL-terminated UTF-16 that outlives the call,
        // which copies the name into the window station's atom table.
        // Registering a name that is already there answers the same id rather
        // than failing, so this is idempotent.
        let id = unsafe { ffi::RegisterClipboardFormatW(wide.as_ptr()) };
        if id == 0 {
            // SAFETY: reads this thread's last error, set above.
            let error = unsafe { ffi::GetLastError() };
            crcbl_core::log::warn!(
                "RegisterClipboardFormatW({name}) failed with Win32 error {error}; that format \
                 cannot be published or read"
            );
            return None;
        }
        Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_text_is_a_builtin_format_and_every_other_mime_is_registered_by_name() {
        // The mapping a copy that no other application can read comes from.
        // `CF_UNICODETEXT` is the one format Windows already knows; everything
        // else is its own mime string, interned in the window station.
        assert_eq!(encoding_for(MimeType::TextUtf8), Encoding::UnicodeText);
        assert_eq!(
            encoding_for(MimeType::CrcblRon),
            Encoding::Registered("application/x-crcbl+ron"),
            "the spelling is the compatibility contract, not an implementation detail"
        );
        assert_eq!(
            encoding_for(MimeType::UriList),
            Encoding::Registered("text/uri-list")
        );
        assert_eq!(
            encoding_for(MimeType::Other("image/png")),
            Encoding::Registered("image/png")
        );
    }

    #[test]
    fn text_round_trips_through_utf16_including_an_astral_codepoint() {
        // Both directions of the conversion, against a string that has all
        // three widths in it: one byte, three bytes, and a surrogate pair.
        let text = "a あ 🎮";
        let bytes = utf16_nul_bytes(text);
        assert_eq!(
            bytes.len(),
            (text.encode_utf16().count() + 1) * 2,
            "one code unit per UTF-16 unit, plus the terminator"
        );
        assert_eq!(&bytes[bytes.len() - 2..], &[0, 0], "NUL-terminated");
        assert_eq!(utf8_from_utf16_bytes(&bytes), text.as_bytes());

        // The empty string is a terminator and nothing else, and reads back as
        // a successful transfer of nothing rather than as a failure.
        assert_eq!(utf16_nul_bytes(""), vec![0, 0]);
        assert_eq!(utf8_from_utf16_bytes(&[0, 0]), Vec::<u8>::new());
        assert_eq!(utf8_from_utf16_bytes(&[]), Vec::<u8>::new());
    }

    #[test]
    fn a_read_stops_at_the_terminator_and_survives_a_block_without_one() {
        // The `GlobalSize` case: the system may hand back a larger block than
        // was written, and `Clipboard::put` zeroes it, so what arrives after
        // the string is NUL units. Reading them would append U+0000s to every
        // paste.
        let mut padded = utf16_nul_bytes("hi");
        padded.extend_from_slice(&[0; 16]);
        assert_eq!(utf8_from_utf16_bytes(&padded), b"hi");

        // An application that wrote the length exactly and no terminator —
        // permitted, and the whole block is then the string.
        let unterminated: Vec<u8> = "hi".encode_utf16().flat_map(u16::to_ne_bytes).collect();
        assert_eq!(utf8_from_utf16_bytes(&unterminated), b"hi");

        // An odd trailing byte is not half a code unit anybody can use.
        let mut ragged = unterminated.clone();
        ragged.push(0x41);
        assert_eq!(utf8_from_utf16_bytes(&ragged), b"hi");

        // An unpaired surrogate is UTF-16 that UTF-8 cannot represent. It
        // becomes U+FFFD rather than failing the whole paste.
        let lone = [0x3D, 0xD8, 0x69, 0x00, 0x00, 0x00];
        assert_eq!(
            utf8_from_utf16_bytes(&lone),
            "\u{FFFD}i".as_bytes(),
            "one replacement character, and the rest of the string survives"
        );
    }

    #[test]
    fn a_registered_payload_is_terminated_on_the_way_out_and_trimmed_on_the_way_back() {
        // The engine-to-engine path, which has to be byte-exact: a RON blob
        // that came back with the heap's padding on it would not parse.
        let ron = b"(kind:\"node\",id:7)";
        let payload = payload_bytes(Encoding::Registered("application/x-crcbl+ron"), ron);
        assert_eq!(payload.last(), Some(&0), "terminated");
        assert_eq!(trim_trailing_nuls(&payload), ron);

        // With the padding a larger-than-requested allocation leaves behind.
        let mut padded = payload.clone();
        padded.extend_from_slice(&[0; 7]);
        assert_eq!(trim_trailing_nuls(&padded), ron);

        // An empty offer is a successful transfer of nothing, which
        // `ClipboardContent` is explicit is not the same as `Empty`.
        assert_eq!(payload_bytes(Encoding::Registered("x"), b""), vec![0]);
        assert_eq!(trim_trailing_nuls(&[0]), b"");
        assert_eq!(trim_trailing_nuls(&[]), b"");
        assert_eq!(trim_trailing_nuls(b"no nuls here"), b"no nuls here");
    }

    #[test]
    fn a_text_offer_that_is_not_valid_utf8_is_replaced_rather_than_refused() {
        // `ClipboardOffer` does not validate — it says so — and
        // `CF_UNICODETEXT` has no representation for a byte that is not a
        // character. The caller logs it; refusing the whole copy would be
        // worse.
        let payload = payload_bytes(Encoding::UnicodeText, b"ok\xFFbad");
        assert_eq!(utf8_from_utf16_bytes(&payload), "ok\u{FFFD}bad".as_bytes());
    }

    #[test]
    fn a_drop_files_block_is_the_packed_sdk_header_and_a_double_nul_wide_list() {
        // Written out by hand from `shlobj_core.h`, not produced by the
        // builder: pFiles = 20, pt = (0, 0), fNC = FALSE, fWide = TRUE, then
        // `C:\a` and `D:\é` in UTF-16LE, each NUL-terminated, then the NUL
        // that ends the list.
        #[rustfmt::skip]
        let expected: &[u8] = &[
            20, 0, 0, 0,  0, 0, 0, 0,  0, 0, 0, 0,  0, 0, 0, 0,  1, 0, 0, 0,
            b'C', 0, b':', 0, b'\\', 0, b'a', 0, 0, 0,
            b'D', 0, b':', 0, b'\\', 0, 0xE9, 0, 0, 0,
            0, 0,
        ];
        assert_eq!(drop_files_block([r"C:\a", r"D:\é"]), expected);

        // The header agrees with the structure the synthetic-drop tests write,
        // field by field, so the two descriptions of `DROPFILES` cannot drift.
        use super::super::ffi::DropFiles;
        use core::mem::offset_of;
        let block = drop_files_block([]);
        let field = |at: usize| u32::from_le_bytes(block[at..at + 4].try_into().unwrap());
        assert_eq!(DROP_FILES_HEADER as usize, size_of::<DropFiles>());
        assert_eq!(field(offset_of!(DropFiles, p_files)), DROP_FILES_HEADER);
        assert_eq!(field(offset_of!(DropFiles, f_nc)), 0);
        assert_eq!(field(offset_of!(DropFiles, f_wide)), 1, "a wide list");
        assert_eq!(
            &block[DROP_FILES_HEADER as usize..],
            &[0, 0],
            "no paths is the terminating NUL alone"
        );
    }

    #[test]
    fn only_the_uris_that_name_windows_files_reach_the_file_list() {
        let list = b"# a comment\r\n\
            file:///C:/a%20b/c.ron\r\n\
            https://example.com/x\r\n\
            file:///tmp/posix\r\n\
            file://server/share/x\n\
            \r\n\
            file:///C:/a%5Cb\r\n";
        assert_eq!(
            hdrop_paths(list),
            [r"C:\a b\c.ron", r"\\server\share\x"],
            "the URL, the drive-less path and the escaped separator are left out; a bare LF \
             still ends a line"
        );
        assert_eq!(
            hdrop_paths(b"https://example.com/\r\n# only this\r\n"),
            Vec::<String>::new(),
            "nothing nameable is an empty list, which the offer publishes no CF_HDROP for"
        );
    }

    #[test]
    fn the_open_retry_budget_is_bounded_and_is_a_fraction_of_a_frame() {
        // Obligation 4's "within a bounded time", as arithmetic rather than as
        // a sentence: the last attempt is not followed by a sleep, so the whole
        // wait is one less than the attempt count.
        assert_eq!(OPEN_BUDGET, OPEN_RETRY * (OPEN_ATTEMPTS - 1));
        assert_eq!(OPEN_BUDGET, Duration::from_millis(70));
        assert!(
            OPEN_BUDGET < Duration::from_millis(100),
            "a paste that stalls the pump for longer than this is a stutter the \
             user sees: {OPEN_BUDGET:?}"
        );
        const {
            assert!(
                OPEN_ATTEMPTS > 1,
                "one attempt is not a retry, and a single refusal is routine"
            );
        }
    }
}
