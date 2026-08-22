//! `NSPasteboard`: which pasteboard type a mime is carried under, how a dropped
//! `public.file-url` becomes a path, and the calls that talk to the pasteboard
//! server.
//!
//! The mapping and the URL decoding are pure and compile on every host, so the
//! parts that can be got wrong quietly — a type string no other application
//! reads, a percent-encoded name turned into the wrong path — are exercised by
//! `cargo test` on the machine this was written on. Everything that sends a
//! message is [`system`], and that is macOS-only.
//!
//! # `NSPasteboard` hands the bytes over, exactly as Win32 does
//!
//! This is the fourth backend to answer the clipboard obligations and the
//! shapes have differed every time, so the shape is stated first:
//!
//! | Backend | Who holds the payload after a write | What can go wrong |
//! | --- | --- | --- |
//! | X11 | this process, until it loses the selection | `INCR`, and a peer that stops answering |
//! | Wayland | this process, until the compositor cancels the source | focus- and serial-gated; a read may have to be *held* |
//! | Win32 | the window station — `SetClipboardData` takes the memory | `OpenClipboard` refused while another process holds it |
//! | **AppKit** | **the pasteboard server** — `setData:forType:` copies it out | **nothing this backend can name** |
//!
//! So this module has no equivalent of `x11::selection`, no deadline, no retry
//! budget and no state carried across a [`pump`](crate::Shell::pump).
//! `-[NSPasteboard setData:forType:]` copies the bytes to `pbs`, the pasteboard
//! server, and the shell keeps nothing. It is Win32's shape **without Win32's
//! hazard**: there is no exclusive open to be refused, because a pasteboard is
//! not locked — it is versioned, by [`change_count`].
//!
//! # Decision: `owner:nil`, so nothing is provided lazily
//!
//! macOS *does* have the lending shape. `addTypes:owner:` with a non-nil owner
//! declares a type and promises to produce it later, from
//! `pasteboard:provideDataForType:` on that owner. **This backend cannot use
//! it**, for two reasons that are structural rather than a matter of effort, and
//! they are the same two the Win32 backend gives for refusing `WM_RENDERFORMAT`:
//!
//! * **The callback has to be answered from a run-loop turn this backend does
//!   not own.** A reader in another process asks `pbs`, which calls back into
//!   *our* main run loop; between two [`pump`](crate::Shell::pump)s an engine is
//!   rendering and there is no turn to service it in. Every callback in this
//!   backend records and returns — see [`events`](super::events) — and an owner
//!   that deferred the answer to the next pump has already handed the reader
//!   nothing.
//! * **A lazy owner owes the pasteboard a flush before the process exits**, and
//!   must stay alive to be messaged until it does. A shell dropped by a host
//!   application that keeps running would leave `pbs` holding an unretained
//!   pointer to a freed object.
//!
//! Writing eagerly costs one copy of the payload and owes nobody anything
//! afterwards. `declareTypes:owner:` with `owner:nil` is that decision written
//! down in the call itself.
//!
//! # A pasteboard type is a UTI, and only one of the two formats has one
//!
//! X11 negotiates over atoms and Win32 registers a format id per name; macOS
//! names its types with **uniform type identifiers**, and the identifier is a
//! plain string that any process may invent.
//!
//! * [`TextUtf8`](MimeType::TextUtf8) has a system UTI —
//!   [`UTF8_PLAIN_TEXT`], which is what `NSPasteboardTypeString` is defined as
//!   — and using it is what makes a copy paste into TextEdit, Safari and every
//!   other application on the desktop.
//! * Everything else is carried under **its own mime string**, verbatim. That
//!   is a legal type identifier, it is unique to this engine by construction,
//!   and it is byte-for-byte what the Win32 backend registers and what the two
//!   Linux backends offer — so an engine-to-engine copy names the same format
//!   on all four platforms. The alternative, a `dyn.*` identifier synthesized
//!   from the mime by `UTTypeCreatePreferredIdentifierForTag`, is opaque,
//!   version-dependent and reaches nothing this one does not.
//!
//! Offering both at once is the caller's job and the reader picks, which is
//! what `docs/plan/15-windowing.md` specifies.
//!
//! # Decision: drag-and-drop lives here, and on Win32 it does not
//!
//! `win32::dnd` is a separate module because there a drop is a *shell object*
//! delivered as a window message and has nothing to do with the window-station
//! clipboard. Here they are one mechanism: `-[NSDraggingInfo
//! draggingPasteboard]` answers an `NSPasteboard`, read with the same
//! `dataForType:` and `pasteboardItems` calls as the general one. That is the
//! "one implementation, two triggers" `docs/plan/15-windowing.md` states for
//! Wayland and X11, arriving on a third platform — so [`file_urls`] serves a
//! drop and [`get`] serves a paste, from one place.
//!
//! # What a drop is read as, and what is deliberately not
//!
//! [`FILE_URL`] per pasteboard item, which is what Finder and every modern
//! application put on a drag. The value is a percent-encoded `file://` URI, so
//! it goes through [`parse_uri_list`](crate::parse_uri_list) — the RFC 2483
//! parser the Wayland, X11 and browser backends already share — rather than
//! through a fourth copy of the same decoding.
//!
//! Two older shapes are **not** read, and neither is a silent omission:
//!
//! * `NSFilenamesPboardType`, a property list of paths, deprecated since 10.13.
//!   Anything still producing it also produces [`FILE_URL`].
//! * `com.apple.pasteboard.promised-file-url`, a *promised* file — the source
//!   has not written it yet and the receiver has to name a destination
//!   directory for it. That is a file the seam has no way to ask for a
//!   destination for; `docs/backlog.md` carries it.

use std::path::PathBuf;

use crate::MimeType;

/// `NSPasteboardTypeString`, spelled out.
///
/// A literal rather than the `NSString *` global, for the reason the mime
/// spellings in [`clipboard`](crate::clipboard) are literals: the identifier is
/// a **compatibility contract** with every other application on the desktop, so
/// it belongs somewhere a test on a Linux host can read it. Its value has been
/// this string since it was introduced, and a UTI cannot change without
/// breaking every application that stored one.
pub const UTF8_PLAIN_TEXT: &str = "public.utf8-plain-text";

/// `NSPasteboardTypeFileURL` — one dropped file, as a `file://` URI.
///
/// See [`UTF8_PLAIN_TEXT`] for why this is a literal.
pub const FILE_URL: &str = "public.file-url";

/// The pasteboard type a mime is published and read under.
///
/// The whole of the format decision, and pure so that it can be checked without
/// a pasteboard server: getting it wrong means a copy no other application can
/// read, which is invisible until somebody tries.
#[must_use]
pub const fn pasteboard_type(mime: MimeType) -> &'static str {
    match mime {
        MimeType::TextUtf8 => UTF8_PLAIN_TEXT,
        // Its own mime string, which is a legal type identifier and is what the
        // other three backends name the same format with. See the module docs.
        MimeType::CrcblRon | MimeType::UriList | MimeType::Other(_) => mime.as_str(),
    }
}

/// One pasteboard item's [`FILE_URL`] as a local path.
///
/// `None` for anything that is not a file on this machine — a `https://` drag
/// from a browser, an `file://host/…` on another host — because
/// [`parse_uri_list`](crate::parse_uri_list) refuses those, which is the whole
/// reason this goes through it rather than stripping a prefix here. Percent
/// decoding, the `localhost` authority and a name that is not valid UTF-8 are
/// all its answers rather than a fourth copy of them.
///
/// A `public.file-url` value is a single URI, so the first parsed path is the
/// only one there can be.
#[must_use]
pub fn path_from_file_url(url: &str) -> Option<PathBuf> {
    crate::parse_uri_list(url.as_bytes()).into_iter().next()
}

#[cfg(target_os = "macos")]
pub(super) use system::{change_count, clear, declare, file_urls, general, get, put};

/// The half that talks to the pasteboard server.
#[cfg(target_os = "macos")]
mod system {
    use core::ffi::c_void;
    use core::ptr;

    use super::super::ffi::{self, Id, NSInteger, NSUInteger, ObjcBool, Sel};

    /// `+[NSPasteboard generalPasteboard]`, or `None` if this image has no
    /// AppKit.
    ///
    /// The object is a singleton the runtime owns for the life of the process,
    /// so there is nothing to retain and nothing that can dangle — which is why
    /// every caller may ask again rather than caching it on the shell.
    ///
    /// # Safety
    ///
    /// The main thread, with an autorelease pool in scope.
    pub(in super::super) unsafe fn general() -> Option<Id> {
        let class = ffi::class(c"NSPasteboard")?;
        // SAFETY: a class method on a live class, taking nothing and returning
        // the shared pasteboard.
        let pasteboard = unsafe { ffi::msg(class, ffi::sel(c"generalPasteboard")) };
        (!pasteboard.is_null()).then_some(pasteboard)
    }

    /// `-[NSPasteboard changeCount]`.
    ///
    /// **The version, and the only thing that makes a write assertable as a
    /// mechanism.** It increases whenever any process claims the pasteboard, so
    /// a write that silently did nothing and a write that took ownership are
    /// distinguishable by a value rather than by a wall clock — which is the
    /// lesson the Win32 half of P5C paid a CI round trip for, transferred here
    /// rather than relearned.
    ///
    /// # Safety
    ///
    /// `pasteboard` must be a live `NSPasteboard`, on the main thread.
    pub(in super::super) unsafe fn change_count(pasteboard: Id) -> NSInteger {
        // SAFETY: an accessor returning an `NSInteger`.
        let send: unsafe extern "C" fn(Id, Sel) -> NSInteger = unsafe { ffi::msg_send() };
        unsafe { send(pasteboard, ffi::sel(c"changeCount")) }
    }

    /// `-[NSPasteboard clearContents]`, answering the new change count.
    ///
    /// The whole of what "release the clipboard" can mean on this platform: a
    /// pasteboard is content, not an owner to give up, so the only thing a
    /// process can do to what it put there is discard it. Same answer as the
    /// Win32 backend's `EmptyClipboard`, and for the same reason.
    ///
    /// # Safety
    ///
    /// `pasteboard` must be a live `NSPasteboard`, on the main thread.
    pub(in super::super) unsafe fn clear(pasteboard: Id) -> NSInteger {
        // SAFETY: takes nothing and returns the new `changeCount`.
        let send: unsafe extern "C" fn(Id, Sel) -> NSInteger = unsafe { ffi::msg_send() };
        unsafe { send(pasteboard, ffi::sel(c"clearContents")) }
    }

    /// `-[NSPasteboard declareTypes:owner:]` for `types`, answering the new
    /// change count.
    ///
    /// Clears the pasteboard and declares every type in one call — which is
    /// what makes the claim and the declaration atomic, where a `clearContents`
    /// followed by an `addTypes:owner:` leaves a window in which the pasteboard
    /// is ours and holds nothing.
    ///
    /// **`owner:nil`**, deliberately: it is the call that says no type here is
    /// provided lazily. See the [module docs](super).
    ///
    /// Answers `None` when a type string could not be made into an `NSString`
    /// — an interior NUL, which only [`Other`](crate::MimeType::Other) can
    /// carry — or when this image has no `NSMutableArray`.
    ///
    /// # Safety
    ///
    /// `pasteboard` must be a live `NSPasteboard`, on the main thread, with an
    /// autorelease pool in scope.
    pub(in super::super) unsafe fn declare(pasteboard: Id, types: &[&str]) -> Option<NSInteger> {
        let array_class = ffi::class(c"NSMutableArray")?;
        // SAFETY: `+[NSMutableArray array]` returns an autoreleased array,
        // valid for the pool the caller holds.
        let array = unsafe { ffi::msg(array_class, ffi::sel(c"array")) };
        if array.is_null() {
            return None;
        }
        for name in types {
            // SAFETY: an autoreleased string, valid for the same pool.
            let string = unsafe { ffi::nsstring(name) }?;
            // SAFETY: a live mutable array and a live string, which it retains.
            unsafe { ffi::msg1_void(array, ffi::sel(c"addObject:"), string) };
        }
        // SAFETY: a live pasteboard and a live array; `nil` is the documented
        // owner for content that is not provided lazily.
        let send: unsafe extern "C" fn(Id, Sel, Id, Id) -> NSInteger = unsafe { ffi::msg_send() };
        Some(unsafe {
            send(
                pasteboard,
                ffi::sel(c"declareTypes:owner:"),
                array,
                ptr::null_mut(),
            )
        })
    }

    /// `-[NSPasteboard setData:forType:]`.
    ///
    /// `false` when the type could not be named, when `NSData` is missing, or
    /// when the pasteboard refused — which it does when ownership changed
    /// between the [`declare`] above and this call, meaning another process
    /// claimed it in between.
    ///
    /// # Safety
    ///
    /// `pasteboard` must be a live `NSPasteboard` this process has just
    /// declared `kind` on, on the main thread, with an autorelease pool in
    /// scope.
    pub(in super::super) unsafe fn put(pasteboard: Id, kind: &str, bytes: &[u8]) -> bool {
        let Some(data_class) = ffi::class(c"NSData") else {
            return false;
        };
        // SAFETY: an autoreleased string and an autoreleased data object, both
        // valid for the pool the caller holds. `dataWithBytes:length:` **copies**
        // the buffer, so `bytes` need not outlive this call.
        let (Some(kind), data) = (unsafe { ffi::nsstring(kind) }, unsafe {
            let send: unsafe extern "C" fn(Id, Sel, *const c_void, NSUInteger) -> Id =
                ffi::msg_send();
            send(
                data_class,
                ffi::sel(c"dataWithBytes:length:"),
                bytes.as_ptr().cast::<c_void>(),
                bytes.len(),
            )
        }) else {
            return false;
        };
        if data.is_null() {
            return false;
        }
        // SAFETY: a live pasteboard, a live data object and a live string.
        let send: unsafe extern "C" fn(Id, Sel, Id, Id) -> ObjcBool = unsafe { ffi::msg_send() };
        unsafe { send(pasteboard, ffi::sel(c"setData:forType:"), data, kind) != ffi::NO }
    }

    /// `-[NSPasteboard dataForType:]` as bytes.
    ///
    /// `None` means the pasteboard holds nothing in that type, which the seam
    /// calls [`Empty`](crate::ClipboardContent::Empty). A type that *is* there
    /// and carries no bytes answers `Some(vec![])` — a successful transfer of
    /// nothing, which [`ClipboardContent`](crate::ClipboardContent) is explicit
    /// is not the same thing.
    ///
    /// # Safety
    ///
    /// `pasteboard` must be a live `NSPasteboard`, on the main thread, with an
    /// autorelease pool in scope.
    pub(in super::super) unsafe fn get(pasteboard: Id, kind: &str) -> Option<Vec<u8>> {
        // SAFETY: an autoreleased string, valid for the caller's pool.
        let kind = unsafe { ffi::nsstring(kind) }?;
        // SAFETY: a live pasteboard and a live string; the returned `NSData` is
        // autoreleased into the caller's pool, or nil.
        let data = unsafe { ffi::msg1(pasteboard, ffi::sel(c"dataForType:"), kind) };
        if data.is_null() {
            return None;
        }
        // SAFETY: a live `NSData`.
        let length = unsafe { ffi::msg_usize(data, ffi::sel(c"length")) };
        if length == 0 {
            // `-[NSData bytes]` is documented to answer nil for an empty
            // object, so there is nothing to read and nothing has gone wrong.
            return Some(Vec::new());
        }
        // SAFETY: an accessor returning a pointer into a buffer the data object
        // owns, valid until the innermost pool drains — which is after the copy
        // below.
        let send: unsafe extern "C" fn(Id, Sel) -> *const c_void = unsafe { ffi::msg_send() };
        let pointer = unsafe { send(data, ffi::sel(c"bytes")) };
        if pointer.is_null() {
            return Some(Vec::new());
        }
        // SAFETY: `pointer` points at `length` readable bytes — the length the
        // object just reported — and stays valid for the length of the copy.
        Some(unsafe { core::slice::from_raw_parts(pointer.cast::<u8>(), length) }.to_vec())
    }

    /// Every pasteboard item's [`FILE_URL`](super::FILE_URL), in order.
    ///
    /// One item per dropped file, which is how a drag is laid out — so this is
    /// the whole of "what was dropped", and turning the strings into paths is
    /// [`path_from_file_url`](super::path_from_file_url)'s job, on the pure
    /// side where it is testable.
    ///
    /// An item with no file URL is skipped rather than being an error: a drag
    /// may carry a mixture, and a text item alongside two files is a drop of
    /// two files.
    ///
    /// # Safety
    ///
    /// `pasteboard` must be a live `NSPasteboard` — the general one, or a
    /// dragging one — on the main thread, with an autorelease pool in scope.
    pub(in super::super) unsafe fn file_urls(pasteboard: Id) -> Vec<String> {
        // SAFETY: an accessor returning an autoreleased array, or nil for a
        // pasteboard with no items.
        let items = unsafe { ffi::msg(pasteboard, ffi::sel(c"pasteboardItems")) };
        if items.is_null() {
            return Vec::new();
        }
        // SAFETY: a live `NSArray`.
        let count = unsafe { ffi::msg_usize(items, ffi::sel(c"count")) };
        let Some(kind) = (
            // SAFETY: an autoreleased string, valid for the caller's pool.
            unsafe { ffi::nsstring(super::FILE_URL) }
        ) else {
            return Vec::new();
        };
        let mut urls = Vec::with_capacity(count);
        for index in 0..count {
            // SAFETY: `index` is in range for an array of `count` objects.
            let item = unsafe {
                let send: unsafe extern "C" fn(Id, Sel, NSUInteger) -> Id = ffi::msg_send();
                send(items, ffi::sel(c"objectAtIndex:"), index)
            };
            if item.is_null() {
                continue;
            }
            // SAFETY: a live `NSPasteboardItem` and a live type string; the
            // answer is an autoreleased `NSString` or nil.
            let string = unsafe { ffi::msg1(item, ffi::sel(c"stringForType:"), kind) };
            // SAFETY: a live `NSString` or nil, with a pool in scope.
            if let Some(url) = unsafe { ffi::string_from_nsstring(string) } {
                urls.push(url);
            }
        }
        urls
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_text_has_a_system_uti_and_every_other_mime_is_its_own_type() {
        // The mapping a copy no other application can read comes from.
        // `public.utf8-plain-text` is what `NSPasteboardTypeString` is defined
        // as and is what TextEdit reads; everything else is its own mime
        // string, which is what the other three backends name it with too.
        assert_eq!(
            pasteboard_type(MimeType::TextUtf8),
            "public.utf8-plain-text"
        );
        assert_eq!(
            pasteboard_type(MimeType::CrcblRon),
            "application/x-crcbl+ron",
            "the spelling is the cross-platform contract, not an implementation detail"
        );
        assert_eq!(pasteboard_type(MimeType::UriList), "text/uri-list");
        assert_eq!(pasteboard_type(MimeType::Other("image/png")), "image/png");

        // The two constants are the ones AppKit defines, and a typo in either
        // is a format nothing on the desktop recognizes.
        assert_eq!(UTF8_PLAIN_TEXT, "public.utf8-plain-text");
        assert_eq!(FILE_URL, "public.file-url");
        assert_ne!(UTF8_PLAIN_TEXT, MimeType::TextUtf8.as_str());
    }

    #[test]
    fn a_dropped_file_url_becomes_the_path_it_names() {
        // What a Finder drag actually puts in `public.file-url`: an absolute
        // `file://` URI with the spaces and the non-ASCII percent-encoded.
        assert_eq!(
            path_from_file_url("file:///Users/dev/My%20Scene.ron"),
            Some(PathBuf::from("/Users/dev/My Scene.ron"))
        );
        assert_eq!(
            path_from_file_url("file:///tmp/a%2Bb%2Fc.png"),
            Some(PathBuf::from("/tmp/a+b/c.png")),
            "a %2F is a slash in the name, decoded after the URI was split"
        );
        assert_eq!(
            path_from_file_url("file://localhost/tmp/x"),
            Some(PathBuf::from("/tmp/x"))
        );
    }

    #[test]
    fn a_url_that_is_not_a_local_file_is_not_turned_into_a_path() {
        // A drag from Safari carries a URL, and producing `example.com/x` from
        // one would look like it worked until something tried to open it. The
        // refusal is `parse_uri_list`'s, which is exactly why this goes through
        // it rather than stripping a prefix here.
        for hostile in [
            "https://example.com/x",
            "file://remotehost/etc/passwd",
            "file:relative/path",
            "",
            "not a uri at all",
        ] {
            assert_eq!(path_from_file_url(hostile), None, "{hostile}");
        }
    }
}
