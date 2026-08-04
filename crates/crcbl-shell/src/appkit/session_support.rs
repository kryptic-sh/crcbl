//! Checks that need a live `NSApplication` **on the process's main thread**,
//! for `tests/appkit_session.rs` to run.
//!
//! Not part of the seam and not for consumers, on the same terms as
//! [`x11_test_support`](crate::x11_test_support) and
//! [`wayland_test_support`](crate::wayland_test_support): those exist because
//! on Linux the window manager, the input devices and the clipboard owner are
//! other *programs*. This one exists because of a **finding**, and the finding
//! is worth stating before the functions are read.
//!
//! # On macOS a test's thread and app state are part of its preconditions
//!
//! `libtest` supplies neither. It runs every body on a thread it spawns, in a
//! process where `NSApplication` may never have been created — and P5C now has
//! two instances of what that costs:
//!
//! * **M1** found that a `#[test]` can never drive a window, because AppKit
//!   raises off the main thread. That is what made `tests/appkit_session.rs`
//!   exist at all.
//! * **M2** shipped a green `#[test]` asserting every
//!   [`CursorIcon`](crate::CursorIcon)'s `NSCursor` selector exists, and the
//!   macOS runner failed it with `+[NSCursor "arrowCursor"] answered nil`. The
//!   selector table was right; the environment was not one in which an AppKit
//!   *object* can be created.
//!
//! So the rule, with two instances behind it: **the Objective-C runtime is
//! thread-safe and needs no application — an AppKit object needs both.**
//! Building a class, registering a selector and dispatching a message are all
//! fine in a `#[test]`, which is why [`view`](super::view)'s and
//! [`shell`](super::shell)'s runtime suites stay where they are. Asking AppKit
//! for a cursor, a pasteboard or a view is not, and belongs here.
//!
//! # Everything here answers `Result<(), String>`
//!
//! Rather than asserting: the caller is a `harness = false` target whose
//! failures have to name the step they reached, and an error carrying the
//! selector or the value that disagreed is what made the M2 failure
//! diagnosable at a glance. It is the same reason [`Wake`](super::shell) is a
//! named value rather than a duration.

use crate::{CursorIcon, MimeType};

use super::ffi::{self, Id, NSUInteger, Pool, Sel};
use super::pasteboard;
use super::pointer;
use super::view;

/// Every cursor shape this backend names exists in this AppKit, and answers a
/// real object.
///
/// [`cursor_selector`](super::pointer::cursor_selector) is a table of selectors
/// written on a host that has no `NSCursor`; this is the other half. A shape
/// naming a method Apple removed — or one the table spelled wrong — is
/// otherwise a `nil` cursor and a `set` that silently does nothing.
///
/// Moved out of a `#[test]` after the M2 runner failed it; see the
/// [module docs](self).
///
/// # Errors
///
/// A string naming the shape and the selector that did not answer.
pub fn every_cursor_shape_exists() -> Result<(), String> {
    let _pool = Pool::push();
    let Some(class) = ffi::class(c"NSCursor") else {
        return Err("the Objective-C runtime has no NSCursor, so this image has no AppKit".into());
    };
    for shape in [
        CursorIcon::Default,
        CursorIcon::Pointer,
        CursorIcon::Text,
        CursorIcon::Crosshair,
        CursorIcon::Wait,
        CursorIcon::Progress,
        CursorIcon::NotAllowed,
        CursorIcon::Grab,
        CursorIcon::Grabbing,
        CursorIcon::Move,
        CursorIcon::ResizeNorthSouth,
        CursorIcon::ResizeEastWest,
        CursorIcon::ResizeNorthEastSouthWest,
        CursorIcon::ResizeNorthWestSouthEast,
        CursorIcon::ResizeColumn,
        CursorIcon::ResizeRow,
    ] {
        let selector = pointer::cursor_selector(shape);
        // SAFETY: every object answers `respondsToSelector:`, and a class is an
        // object.
        if !unsafe { ffi::responds_to(class, ffi::sel(selector)) } {
            return Err(format!(
                "+[NSCursor {selector:?}] does not exist, so {shape:?} would be nil"
            ));
        }
        // SAFETY: the selector exists on this system and is a class method
        // taking nothing and returning the shared cursor.
        let cursor = unsafe { ffi::msg(class, ffi::sel(selector)) };
        if cursor.is_null() {
            return Err(format!("+[NSCursor {selector:?}] answered nil"));
        }
    }
    Ok(())
}

/// A `CrcblView` registers for exactly the dragged type this backend reads, and
/// an unregistered one is registered for nothing.
///
/// **This is the drop gate as a mechanism rather than as a promise.** No
/// `NSDraggingDestination` message is ever sent to a view that has not called
/// `registerForDraggedTypes:` — so
/// [`WindowDesc::accept_drops`](crate::WindowDesc::accept_drops) being off is
/// enforced by the system, exactly as `WS_EX_ACCEPTFILES` enforces it on Win32
/// and unlike Wayland, where the backend has to refuse the offer itself. Both
/// halves are checked, because a registration that happened unconditionally
/// would pass a test that only looked at the registered view.
///
/// The views are built and thrown away here rather than taken from a window:
/// the registration is a property of the view, so a window would add nothing
/// but a dependency on one existing.
///
/// # Errors
///
/// A string naming what each view was registered for instead.
pub fn dragged_types_register_on_a_view() -> Result<(), String> {
    let _pool = Pool::push();
    let class = view::view_class().map_err(|error| error.to_string())?;
    // SAFETY: a registered class; `alloc` then `init` is how an `NSView` is
    // made, and both are released below.
    let bare = unsafe {
        let allocated = ffi::msg(class, ffi::sel(c"alloc"));
        ffi::msg(allocated, ffi::sel(c"init"))
    };
    // SAFETY: as above.
    let registered = unsafe {
        let allocated = ffi::msg(class, ffi::sel(c"alloc"));
        ffi::msg(allocated, ffi::sel(c"init"))
    };
    if bare.is_null() || registered.is_null() {
        return Err("[[CrcblView alloc] init] was nil".into());
    }
    // SAFETY: a live view of this backend's own class, on the main thread with
    // the pool above in scope.
    unsafe { view::register_dragged_types(registered) };

    // SAFETY: both are live views of the class built above.
    let before = unsafe { registered_types(bare) };
    // SAFETY: as above.
    let after = unsafe { registered_types(registered) };
    // SAFETY: one release each for the one retain `alloc` gave them; neither is
    // used afterwards.
    unsafe {
        ffi::msg_void(bare, ffi::sel(c"release"));
        ffi::msg_void(registered, ffi::sel(c"release"));
    }

    if !before.is_empty() {
        return Err(format!(
            "a CrcblView that was never registered already accepts drags of {before:?}, so \
             accept_drops: false would gate nothing"
        ));
    }
    if after != [pasteboard::FILE_URL] {
        return Err(format!(
            "registerForDraggedTypes: left the view accepting {after:?} rather than only {:?}, \
             so a file drop would not be delivered",
            pasteboard::FILE_URL
        ));
    }
    Ok(())
}

/// `-[NSView registeredDraggedTypes]` as strings.
///
/// # Safety
///
/// `view` must be a live `NSView`, on the main thread, with an autorelease pool
/// in scope.
unsafe fn registered_types(view: Id) -> Vec<String> {
    // SAFETY: an accessor returning an autoreleased array, which may be nil.
    let types = unsafe { ffi::msg(view, ffi::sel(c"registeredDraggedTypes")) };
    if types.is_null() {
        return Vec::new();
    }
    // SAFETY: a live `NSArray`.
    let count = unsafe { ffi::msg_usize(types, ffi::sel(c"count")) };
    let mut names = Vec::with_capacity(count);
    for index in 0..count {
        // SAFETY: `index` is in range for an array of `count` objects, each of
        // which is an `NSString` a registration put there.
        let name = unsafe {
            let send: unsafe extern "C" fn(Id, Sel, NSUInteger) -> Id = ffi::msg_send();
            let string = send(types, ffi::sel(c"objectAtIndex:"), index);
            ffi::string_from_nsstring(string)
        };
        if let Some(name) = name {
            names.push(name);
        }
    }
    names
}

/// The general pasteboard takes a two-format write, gives both back, and its
/// **change count advances**.
///
/// # Why the change count and not the bytes alone
///
/// Reading back what was written is satisfied by a backend answering its own
/// reads out of a cache, and by a write that was refused while the previous
/// contents happened to say the same thing. `-[NSPasteboard changeCount]` is
/// the pasteboard's own version: it increases when a process **claims** the
/// pasteboard, so an advance is evidence that `declareTypes:owner:` took
/// ownership rather than merely returning. That is the mechanism, and the
/// values it disagreed by are in the failure — the shape the Win32 half of P5C
/// found the hard way, where a failure printing a real state cost one CI round
/// trip and one printing a duration cost three.
///
/// **This writes to the runner's real system pasteboard**, which is a side
/// effect and is also the point: it is the same object every other application
/// on the machine reads, so a round trip through it is the strongest statement
/// this slice can make. There is nothing to restore afterwards — a pasteboard
/// has no previous version to put back, which is exactly what
/// [`clipboard_offer`](crate::Shell::clipboard_offer) documents for this
/// platform.
///
/// # Errors
///
/// A string carrying the change counts and whatever came back instead.
pub fn pasteboard_round_trip() -> Result<(), String> {
    const TEXT: &str = "crcbl M3 — pasteboard 🎮";
    const RON: &[u8] = b"(kind:\"node\",id:7)";

    let _pool = Pool::push();
    // SAFETY: the main thread, with the pool above in scope.
    let Some(pasteboard) = (unsafe { pasteboard::general() }) else {
        return Err("+[NSPasteboard generalPasteboard] answered nil".into());
    };
    // SAFETY: a live pasteboard.
    let before = unsafe { pasteboard::change_count(pasteboard) };

    let text_type = pasteboard::pasteboard_type(MimeType::TextUtf8);
    let ron_type = pasteboard::pasteboard_type(MimeType::CrcblRon);
    // SAFETY: a live pasteboard, on the main thread, with a pool in scope.
    let declared = unsafe { pasteboard::declare(pasteboard, &[text_type, ron_type]) }
        .ok_or_else(|| "declareTypes:owner: could not be built".to_string())?;
    if declared <= before {
        return Err(format!(
            "declareTypes:owner: left the change count at {declared} from {before}, so nothing \
             claimed the pasteboard"
        ));
    }
    // SAFETY: as above, and both types have just been declared on it.
    unsafe {
        if !pasteboard::put(pasteboard, text_type, TEXT.as_bytes()) {
            return Err(format!("setData:forType:{text_type} was refused"));
        }
        if !pasteboard::put(pasteboard, ron_type, RON) {
            return Err(format!("setData:forType:{ron_type} was refused"));
        }
    }

    // SAFETY: as above.
    let text = unsafe { pasteboard::get(pasteboard, text_type) };
    // SAFETY: as above.
    let ron = unsafe { pasteboard::get(pasteboard, ron_type) };
    // A type nobody published: an *empty* answer rather than an error, which is
    // the distinction `ClipboardContent` exists to keep.
    // SAFETY: as above.
    let absent = unsafe { pasteboard::get(pasteboard, "sh.kryptic.crcbl.no-such-type") };
    // SAFETY: as above.
    let after = unsafe { pasteboard::change_count(pasteboard) };

    if text.as_deref() != Some(TEXT.as_bytes()) {
        return Err(format!(
            "the general pasteboard answered {:?} for {text_type} rather than what was written",
            text.as_deref().map(String::from_utf8_lossy)
        ));
    }
    if ron.as_deref() != Some(RON) {
        return Err(format!(
            "the engine's own format did not survive the round trip: {:?}",
            ron.as_deref().map(String::from_utf8_lossy)
        ));
    }
    if absent.is_some() {
        return Err(format!("a type nobody wrote answered {absent:?}"));
    }
    if after != declared {
        return Err(format!(
            "the change count moved from {declared} to {after} while this process owned the \
             pasteboard, so something else claimed it mid-check"
        ));
    }
    Ok(())
}
