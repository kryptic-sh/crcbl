//! Scaffolding for the AppKit window session, `tests/appkit_session.rs`.
//!
//! What it holds is the checks that need a live `NSApplication` **on the
//! process's main thread** — a distinction P5C has now paid for twice, and one
//! this module states as a rule.
//!
//! Not part of the seam and not for consumers, on the same terms as
//! `x11_test_support` and `wayland_test_support`: those exist because on Linux
//! the window manager, the input devices and the clipboard owner are other
//! *programs*. This one exists because of a **finding**, and the finding is
//! worth stating before the functions are read. It is behind no feature, unlike
//! those two, because the session target is behind none either: `libtest`
//! always runs a body on a thread it spawns and AppKit is main-thread-only, so
//! that target owns its `main` and there is no version of it that a feature
//! flag could turn off.
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
//! * **M2** shipped a green `#[test]` asserting every [`CursorIcon`]'s
//!   `NSCursor` selector exists, and the macOS runner failed it with
//!   `+[NSCursor "arrowCursor"] answered nil`. The selector table was right;
//!   the environment was not one in which an AppKit *object* can be created.
//!
//! So the rule, with two instances behind it: **the Objective-C runtime is
//! thread-safe and needs no application — an AppKit object needs both.**
//! Building a class, registering a selector and dispatching a message are all
//! fine in a `#[test]`, which is why `view`'s and `shell`'s runtime suites stay
//! where they are. Asking AppKit for a cursor, a pasteboard or a view is not,
//! and belongs here.
//!
//! # Everything here answers `Result<(), String>`
//!
//! Rather than asserting: the caller is a `harness = false` target whose
//! failures have to name the step they reached, and an error carrying the
//! selector or the value that disagreed is what made the M2 failure diagnosable
//! at a glance. It is the same reason `shell::Wake` is a named value rather
//! than a duration.

use core::ffi::{CStr, c_char};

use crate::{CursorIcon, MimeType};

use super::ffi::{self, Id, NSSize, NSUInteger, Pool, Sel, style};
use super::pasteboard;
use super::pointer;
use super::view;

// The one runtime entry point this module needs that the backend itself has no
// use for: the *name* of an object's class. Everything else here goes through
// [`ffi`]; this is declared beside its only caller rather than added to that
// table, which is deliberately audited by use.
#[link(name = "objc")]
unsafe extern "C" {
    /// The class name of an object, as a NUL-terminated C string owned by the
    /// runtime.
    fn object_getClassName(object: Id) -> *const c_char;
}

/// Every cursor shape this backend names exists in this AppKit, and answers a
/// real object.
///
/// `pointer::cursor_selector` is a table of selectors written on a host that
/// has no `NSCursor`; this is the other half. A shape naming a method Apple
/// removed — or one the table spelled wrong — is otherwise a `nil` cursor and a
/// `set` that silently does nothing.
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

// ---------------------------------------------------------------------------
// The window this process opened, as AppKit describes it
// ---------------------------------------------------------------------------

/// Why no window of this process has the keyboard.
///
/// # It separates our defect from the runner's refusal, and those need
/// different answers
///
/// "There is no key window" has two completely different causes and one
/// symptom, which is the shape of failure P5C has already paid for four times
/// on Windows: no way to tell which mechanism produced it.
///
/// * **`can_become_key` is false on our own window** — that is *our* bug.
///   `CrcblWindow` overrides `canBecomeKeyWindow` to answer `YES` precisely
///   because a borderless `NSWindow` answers `NO` by default, so a false here
///   means the override is not installed on the class the window actually is.
///   A game would go borderless and silently stop receiving keystrokes.
/// * **`app_active` is false while `can_become_key` is true** — the *session*
///   refused to activate us. An unbundled binary on a machine with nobody
///   logged in at the console is the case that matters here, and there is
///   nothing a backend can do about it.
///
/// **The macOS runner answered: the second one.** `can_become_key: true`,
/// `visible: true`, one window with the right title and the right class, and
/// `app_active: false`. So this is the diagnostic that did its job, and what it
/// found is a property of the environment rather than of the backend — which is
/// why [`WindowFacts`] no longer goes through the key window to read anything it
/// does not have to.
///
/// Nothing branches on this in the backend; it exists so a failure or a skip can
/// say which of the two happened rather than leaving the next reader to guess.
#[derive(Clone, Debug, PartialEq)]
pub struct Activation {
    /// `-[NSApp isActive]`.
    pub app_active: bool,
    /// How many windows `-[NSApp windows]` holds. Zero means the shell's window
    /// never reached the application, which is a third failure again.
    pub windows: usize,
    /// `-[NSWindow title]` of the window that was asked for, or `None` when no
    /// window of this process carries that title — which [`windows`](Self::windows)
    /// tells apart from there being no windows at all.
    pub title: Option<String>,
    /// The class name of that window — `CrcblWindow` if the subclass took.
    pub class: String,
    /// `-[NSWindow isVisible]`.
    pub visible: bool,
    /// `-[NSWindow canBecomeKeyWindow]`, the one that separates our bug from
    /// the environment's refusal.
    pub can_become_key: bool,
    /// `-[NSWindow isKeyWindow]`.
    pub is_key: bool,
}

/// What the application and the window titled `title` say about taking the
/// keyboard.
///
/// See [`Activation`] for why the two halves are reported separately.
///
/// # Errors
///
/// A string when there is no `NSApplication` at all, which means the shell was
/// never opened in this process. A window that is simply not there is **not** an
/// error: it is an `Activation` with `title: None` and a window count, because
/// that is evidence rather than a failure to gather it.
pub fn activation(title: &str) -> Result<Activation, String> {
    let _pool = Pool::push();
    let app = shared_application()?;
    // SAFETY: a live `NSApplication`; the accessor takes no arguments.
    let app_active = unsafe { ffi::msg_bool(app, ffi::sel(c"isActive")) };
    // SAFETY: as above, on the main thread with the pool in scope.
    let windows = unsafe { windows_of(app) };
    // SAFETY: every element is a live `NSWindow` the array owns.
    let found = windows
        .iter()
        .copied()
        .find(|window| unsafe { title_of(*window) }.as_deref() == Some(title));
    let Some(window) = found else {
        return Ok(Activation {
            app_active,
            windows: windows.len(),
            title: None,
            class: "nil".to_owned(),
            visible: false,
            can_become_key: false,
            is_key: false,
        });
    };
    // SAFETY: a live `NSWindow`. `title` answers an `NSString` the window owns;
    // the three predicates take no arguments and answer `BOOL`.
    let (class, visible, can_become_key, is_key) = unsafe {
        (
            class_name(window),
            ffi::msg_bool(window, ffi::sel(c"isVisible")),
            ffi::msg_bool(window, ffi::sel(c"canBecomeKeyWindow")),
            ffi::msg_bool(window, ffi::sel(c"isKeyWindow")),
        )
    };
    Ok(Activation {
        app_active,
        windows: windows.len(),
        title: Some(title.to_owned()),
        class,
        visible,
        can_become_key,
        is_key,
    })
}

/// What AppKit says about a window of this process, read off the **window**.
///
/// # Why the seam is not the judge here
///
/// Every field below is something [`Shell`](crate::Shell) already has an opinion
/// about, and that is the point: a backend's `window_state` is *its own record of
/// what it asked for*, so a test comparing the two is comparing a claim with
/// itself. These come out of `NSApplication`, `NSWindow` and `NSView` accessors
/// instead, which is the same standard the Win32 suite holds itself to with
/// `GetClientRect` and `GetForegroundWindow`.
///
/// # Three of the five switches become mechanisms here
///
/// `view` lists five things that have to be switched on before a real event
/// exists, and says they are structural claims rather than observed behaviour.
/// Three of them have a readback on a **live** window and are in this struct:
/// [`accepts_mouse_moved`](Self::accepts_mouse_moved),
/// [`first_responder`](Self::first_responder) — which has to be `CrcblView` and
/// not the window — and [`dragged_types`](Self::dragged_types). Reading them
/// off the window a shell actually created is what turns "we called the setter"
/// into "the window is in that state".
///
/// # None of it needs the keyboard, and that is the finding this shape exists for
///
/// The first version of this reached its window through `-[NSApp keyWindow]`,
/// which is nil for an application that never became active — and that is
/// exactly what a GitHub macOS runner does to an unbundled binary. So the whole
/// readback above, every one of which is an ordinary accessor any window
/// answers, was being thrown away over a precondition that only injected input
/// actually has. [`window_facts`] finds the window by title among
/// `-[NSApp windows]` instead, and [`app_active`](Self::app_active) and
/// [`is_key`](Self::is_key) are *reported* rather than required.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowFacts {
    /// `-[NSApp isActive]`. Reported, never a precondition: see the type's docs.
    pub app_active: bool,
    /// `-[NSWindow isKeyWindow]`. Reported for the same reason — it is the one
    /// thing `CGEventPost` needs and nothing else here does.
    pub is_key: bool,
    /// `-[NSWindow title]`, which is how a caller tells its own window from
    /// somebody else's.
    pub title: String,
    /// `-[NSWindow acceptsMouseMovedEvents]`. **The system generates no
    /// `NSEventTypeMouseMoved` at all for a window that answers `false`**, so
    /// this is pointer motion existing or not existing.
    pub accepts_mouse_moved: bool,
    /// The class name of `-[NSWindow firstResponder]`.
    ///
    /// `sendEvent:` delivers key events here. It is the *window* until something
    /// claims it, and a window that is its own first responder receives every
    /// keystroke and hands the view none.
    pub first_responder: String,
    /// The class name of `-[NSWindow contentView]`.
    pub content_view: String,
    /// `-[NSView registeredDraggedTypes]` on the content view.
    ///
    /// Empty for a window created without
    /// [`accept_drops`](crate::WindowDesc::accept_drops), and that is the gate:
    /// AppKit sends no `NSDraggingDestination` message to an unregistered view.
    pub dragged_types: Vec<String>,
    /// `-[NSWindow styleMask]`, for a failure message.
    pub style_mask: usize,
    /// Whether that mask has `NSWindowStyleMaskTitled` in it, which is what
    /// separates a windowed window from a borderless one.
    pub titled: bool,
    /// `-[NSWindow frame]` as `[x, y, width, height]`, in AppKit screen points
    /// with **Y up**.
    pub frame: [f64; 4],
    /// `-[[NSWindow contentView] frame].size` in points, which is the extent a
    /// swapchain covers before the backing scale is applied.
    pub content_points: [f64; 2],
    /// `-[NSWindow backingScaleFactor]`.
    pub backing_scale: f64,
    /// `-[[NSWindow screen] frame]`, or `None` for a window on no screen.
    pub screen_frame: Option<[f64; 4]>,
}

/// This process's own window, found by `title`, described from AppKit's own
/// accessors.
///
/// **This is the entry point for everything that does not need the keyboard**,
/// which is everything but injection — see [`WindowFacts`].
///
/// # Errors
///
/// A string when there is no `NSApplication`, or when no window of this process
/// carries that title — carrying the count and the titles that were there
/// instead, since "the shell never registered its window" and "somebody renamed
/// it" are different findings.
pub fn window_facts(title: &str) -> Result<WindowFacts, String> {
    let _pool = Pool::push();
    let app = shared_application()?;
    // SAFETY: a live `NSApplication`; the accessor takes no arguments.
    let app_active = unsafe { ffi::msg_bool(app, ffi::sel(c"isActive")) };
    // SAFETY: as above, on the main thread with the pool in scope.
    let window = unsafe { window_titled(app, title) }?;
    // SAFETY: a live `NSWindow` this process owns, main thread, pool in scope.
    Ok(unsafe { facts_of(window, app_active) })
}

/// The window `-[NSApp keyWindow]` names, described the same way.
///
/// # The one thing that genuinely needs the key window
///
/// `CGEventPost` hands an event to the **session**, which delivers it to
/// whatever holds the keyboard — so posting while another application has the
/// key window types into that application, which on a CI runner is the job's own
/// shell. That check is this function's only remaining caller.
///
/// Everything else reads its window through [`window_facts`] and needs no
/// activation at all, which is the M5 correction: this used to gate the entire
/// readback, and on a runner that never activates it threw all of it away.
///
/// # Errors
///
/// A string saying whether the application is active, which is the half that
/// separates a session refusing to activate an unbundled binary from a window
/// that was never eligible. [`activation`] separates them in full.
pub fn key_window() -> Result<WindowFacts, String> {
    let _pool = Pool::push();
    let app = shared_application()?;
    // SAFETY: a live `NSApplication`; both accessors take nothing.
    let (app_active, window) = unsafe {
        (
            ffi::msg_bool(app, ffi::sel(c"isActive")),
            ffi::msg(app, ffi::sel(c"keyWindow")),
        )
    };
    if window.is_null() {
        return Err(format!(
            "-[NSApp keyWindow] is nil and the application is {}active, so no window of this \
             process has the keyboard; injected input would go to whatever does",
            if app_active { "" } else { "in" }
        ));
    }
    // SAFETY: a live `NSWindow`, main thread, pool in scope.
    Ok(unsafe { facts_of(window, app_active) })
}

/// Resizes the **content area** of this process's window titled `title`, in
/// AppKit points.
///
/// `setContentSize:` rather than `setFrame:display:`, and the difference is the
/// whole reason this is a function rather than a line in the caller: a frame
/// includes the title bar, so a test that set one and compared the seam's
/// reported size against it would be off by the frame's height on a windowed
/// window and correct on a borderless one — which is the shape of bug that
/// passes in one mode and is blamed on the other.
///
/// This is what a user dragging the window's edge produces, minus the modal
/// drag loop. It is here rather than in the test because it needs the
/// `NSWindow`, and the seam hands out a `CAMetalLayer`.
///
/// # Errors
///
/// A string naming what was missing, on the same terms as [`window_facts`].
pub fn resize_window(title: &str, width: f64, height: f64) -> Result<(), String> {
    let _pool = Pool::push();
    let app = shared_application()?;
    // SAFETY: a live `NSApplication`, main thread, pool in scope.
    let window = unsafe { window_titled(app, title) }?;
    // SAFETY: a live `NSWindow`; `setContentSize:` takes an `NSSize` by value.
    unsafe {
        ffi::msg_set_size(
            window,
            ffi::sel(c"setContentSize:"),
            NSSize { width, height },
        );
    }
    Ok(())
}

/// The `NSApplication` singleton.
///
/// # Errors
///
/// A string when this image has no AppKit, or when the singleton could not be
/// created — which is what a process with no WindowServer session gets.
fn shared_application() -> Result<Id, String> {
    let Some(class) = ffi::class(c"NSApplication") else {
        return Err("the Objective-C runtime has no NSApplication".into());
    };
    // SAFETY: a class method returning the singleton, which the runtime keeps
    // for the life of the process.
    let app = unsafe { ffi::msg(class, ffi::sel(c"sharedApplication")) };
    if app.is_null() {
        return Err("[NSApplication sharedApplication] answered nil".into());
    }
    Ok(app)
}

/// Every window `-[NSApp windows]` holds.
///
/// # Safety
///
/// `app` must be the live `NSApplication`, on the main thread, with an
/// autorelease pool in scope. The windows are borrowed from the array and are
/// valid until that pool drains.
unsafe fn windows_of(app: Id) -> Vec<Id> {
    // SAFETY: an accessor answering an `NSArray`, or nil before any window
    // exists.
    let windows = unsafe { ffi::msg(app, ffi::sel(c"windows")) };
    if windows.is_null() {
        return Vec::new();
    }
    // SAFETY: a live `NSArray`.
    let count = unsafe { ffi::msg_usize(windows, ffi::sel(c"count")) };
    (0..count)
        .map(|index| {
            // SAFETY: `index` is in range for an array of `count` objects, and
            // the array owns what it answers.
            unsafe {
                let send: unsafe extern "C" fn(Id, Sel, NSUInteger) -> Id = ffi::msg_send();
                send(windows, ffi::sel(c"objectAtIndex:"), index)
            }
        })
        .collect()
}

/// The one window of this process whose `-[NSWindow title]` is `title`.
///
/// # Errors
///
/// A string carrying every title that was there instead, which is what makes a
/// miss diagnosable in one round rather than two.
///
/// # Safety
///
/// As [`windows_of`].
unsafe fn window_titled(app: Id, title: &str) -> Result<Id, String> {
    // SAFETY: the caller's obligation, forwarded.
    let windows = unsafe { windows_of(app) };
    // SAFETY: every element is a live `NSWindow` the array owns.
    let titles: Vec<Option<String>> = windows
        .iter()
        .map(|window| unsafe { title_of(*window) })
        .collect();
    windows
        .iter()
        .zip(&titles)
        .find(|(_, seen)| seen.as_deref() == Some(title))
        .map(|(window, _)| *window)
        .ok_or_else(|| {
            format!(
                "no window of this process is titled {title:?}; -[NSApp windows] holds {} and \
                 they are titled {titles:?}",
                windows.len()
            )
        })
}

/// `-[NSWindow title]`, or `None` for a window with none.
///
/// # Safety
///
/// `window` must be a live `NSWindow`, on the main thread, with an autorelease
/// pool in scope.
unsafe fn title_of(window: Id) -> Option<String> {
    // SAFETY: `title` answers an `NSString` the window owns; the contents are
    // copied out here rather than borrowed past the pool.
    unsafe { ffi::string_from_nsstring(ffi::msg(window, ffi::sel(c"title"))) }
}

/// Everything [`WindowFacts`] carries, read off `window`.
///
/// `app_active` is passed in rather than read here because it is a property of
/// the *application* and every caller has already asked for it.
///
/// # Safety
///
/// `window` must be a live `NSWindow`, on the main thread, with an autorelease
/// pool in scope.
unsafe fn facts_of(window: Id, app_active: bool) -> WindowFacts {
    // SAFETY: a live `NSWindow`. Each is an accessor of the type its helper
    // dispatches, and the two object accessors answer objects owned by the
    // window rather than by this call.
    let (is_key, accepts_mouse_moved, responder, content, style_mask, frame, backing_scale, screen) = unsafe {
        (
            ffi::msg_bool(window, ffi::sel(c"isKeyWindow")),
            ffi::msg_bool(window, ffi::sel(c"acceptsMouseMovedEvents")),
            ffi::msg(window, ffi::sel(c"firstResponder")),
            ffi::msg(window, ffi::sel(c"contentView")),
            ffi::msg_usize(window, ffi::sel(c"styleMask")),
            ffi::msg_rect(window, ffi::sel(c"frame")),
            ffi::msg_f64(window, ffi::sel(c"backingScaleFactor")),
            ffi::msg(window, ffi::sel(c"screen")),
        )
    };
    // SAFETY: as above.
    let title = unsafe { title_of(window) }.unwrap_or_default();
    // SAFETY: a live `NSView` — the window's own — and `frame` is its accessor.
    let content_frame = unsafe { ffi::msg_rect(content, ffi::sel(c"frame")) };
    // SAFETY: as above; `registered_types` documents what it needs and both
    // conditions hold here.
    let dragged_types = unsafe { registered_types(content) };
    let screen_frame = (!screen.is_null()).then(|| {
        // SAFETY: a live `NSScreen`, which the window owns a reference to.
        let frame = unsafe { ffi::msg_rect(screen, ffi::sel(c"frame")) };
        [
            frame.origin.x,
            frame.origin.y,
            frame.size.width,
            frame.size.height,
        ]
    });

    WindowFacts {
        app_active,
        is_key,
        title,
        accepts_mouse_moved,
        // SAFETY of both: a live object; the runtime owns the name it answers
        // and it is copied out before this pool is popped.
        first_responder: unsafe { class_name(responder) },
        content_view: unsafe { class_name(content) },
        dragged_types,
        style_mask,
        titled: style_mask & style::TITLED != 0,
        frame: [
            frame.origin.x,
            frame.origin.y,
            frame.size.width,
            frame.size.height,
        ],
        content_points: [content_frame.size.width, content_frame.size.height],
        backing_scale,
        screen_frame,
    }
}

/// An object's class name.
///
/// # Safety
///
/// `object` must be a live Objective-C object, or null — which answers `"nil"`
/// rather than reading anything.
unsafe fn class_name(object: Id) -> String {
    if object.is_null() {
        return "nil".to_owned();
    }
    // SAFETY: the caller guarantees a live object; the runtime owns the
    // returned string, which is NUL-terminated and outlives this borrow.
    let name = unsafe { object_getClassName(object) };
    if name.is_null() {
        return "?".to_owned();
    }
    // SAFETY: a NUL-terminated string the runtime keeps for the life of the
    // class, copied out here rather than borrowed.
    unsafe { CStr::from_ptr(name) }
        .to_string_lossy()
        .into_owned()
}
