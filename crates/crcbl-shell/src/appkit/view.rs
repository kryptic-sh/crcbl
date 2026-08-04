//! The `NSView` subclass this backend builds at runtime: every responder method
//! input arrives through, the tracking area, and the `NSTextInputClient` that
//! makes text exist at all.
//!
//! # Decision: a view subclass, not an interception in the pump
//!
//! [`pump`](crate::Shell::pump) dequeues each `NSEvent` and hands it to
//! `-[NSApplication sendEvent:]`, so it *could* read `[event type]` and take the
//! input apart there instead — several engines do. This backend does not, for
//! three reasons that are all about what `sendEvent:` knows and a bystander does
//! not:
//!
//! 1. **`sendEvent:` is what routes an event to a window**, and a shell with two
//!    windows would otherwise have to reimplement that routing from
//!    `[event window]` — including the cases where AppKit sends an event
//!    somewhere else, which is exactly what happens to a key equivalent.
//! 2. **`mouseEntered:`/`mouseExited:` have no `NSEvent` in the queue to
//!    intercept.** They are delivered by the tracking area, to an owner, and the
//!    owner has to be an object.
//! 3. **Text is not in the event at all.** It comes back out of
//!    `interpretKeyEvents:` through a responder, which means an object again.
//!
//! So there is a class, and the events reach the shell the way everything else
//! in this backend does: recorded into [`Shared`](super::app::Shared) and
//! translated later. The view carries no instance storage, exactly as
//! [`app`](super::app)'s delegate does not — it finds its shell through
//! `[[self window] delegate]`, which is the delegate object that *is* in the
//! registry.
//!
//! # The AppKit shape of the `TranslateMessage` gap, and it is four gaps
//!
//! The Win32 half of P5C shipped a backend whose in-crate tests all passed and
//! whose `TextCommit` was unreachable from a real keyboard, because the pump
//! never called `TranslateMessage`. The equivalent question here — *what does
//! the real event path do that a hand-constructed call to a responder method
//! would skip?* — has four answers, and every one of them is invisible to a test
//! that calls `keyDown:` itself:
//!
//! | Missing | What silently stops working | Why a direct call cannot see it |
//! | --- | --- | --- |
//! | `-[NSWindow setAcceptsMouseMovedEvents:YES]` | **all** pointer motion with no button held | the system never *generates* `NSEventTypeMouseMoved` for a window that has not asked; `mouseMoved:` called by hand works perfectly |
//! | `-[NSWindow makeFirstResponder:view]` | every key event | `sendEvent:` delivers key events to the first responder, which is the window itself by default; a direct `keyDown:` bypasses the question |
//! | the `NSTrackingArea` | `PointerFocus` in both directions | nothing else produces `mouseEntered:`, so a test that calls it is testing its own call |
//! | `interpretKeyEvents:` | **all** text, including dead keys | `insertText:` is called *by the input method*; a test that calls it is the input method |
//!
//! All four are established in [`window`](super::window) and here, and the
//! honest statement about them is in `docs/backlog.md`: they are structural
//! rather than verified, because verifying them needs injected input at the
//! session level, which is M4.
//!
//! # Decision: a real `NSTextInputClient`, not `-[NSEvent characters]`
//!
//! The short path to text is to read `characters` off the key event and be done.
//! It produces the right string for an unmodified letter and the wrong one — or
//! none — for everything that composes: a dead key's state lives in the
//! *input context*, not in the event, so `⌥e` followed by `e` yields `e` rather
//! than `é`, and an input method never gets to run at all. Half of Europe cannot
//! type its own language against that.
//!
//! `interpretKeyEvents:` is the documented path, and it requires
//! `-[NSResponder inputContext]` to be non-nil, which requires the responder to
//! **conform to `NSTextInputClient`** — eleven methods, all of them required.
//! They are implemented below. What is deliberately not implemented is
//! *displaying* the pre-edit: the seam has no pre-edit event, so marked text is
//! tracked because an input method cannot compose without the answers and is
//! never surfaced. `firstRectForCharacterRange:` answers with the window's own
//! origin rather than a caret the seam does not model, so a candidate window
//! appears at the bottom-left of the window rather than under the text.
//!
//! `doCommandBySelector:` is overridden to do **nothing**, which is not laziness:
//! the default implementation beeps at every command it does not recognize, so a
//! game whose player presses Escape or an arrow key would beep once per
//! keystroke.

use core::ffi::CStr;
use core::ptr;
use std::sync::OnceLock;

use crcbl_core::input::ButtonState;

use super::app;
use super::events::RawEvent;
use super::ffi::{self, Class, Id, Imp, NSPoint, NSRange, NSRect, NSUInteger, ObjcBool, Sel, YES};
use super::keys;

/// The name of the `NSView` subclass this module builds at runtime.
const VIEW_CLASS: &CStr = c"CrcblView";

/// The registration, once per process. `Err` carries what went wrong.
static VIEW: OnceLock<Result<usize, String>> = OnceLock::new();

// ---------------------------------------------------------------------------
// Reading an NSEvent
// ---------------------------------------------------------------------------

/// `-[NSEvent timestamp]`: seconds since the system booted.
///
/// # Safety
///
/// `event` must be a live `NSEvent`.
unsafe fn seconds(event: Id) -> f64 {
    // SAFETY: an `NSEvent` accessor returning an `NSTimeInterval`, which is a
    // `double`.
    unsafe { ffi::msg_f64(event, ffi::sel(c"timestamp")) }
}

/// `-[NSEvent modifierFlags]`.
///
/// # Safety
///
/// `event` must be a live `NSEvent`.
unsafe fn flags(event: Id) -> NSUInteger {
    // SAFETY: an `NSEvent` accessor returning an `NSEventModifierFlags`, which
    // is an `NSUInteger`.
    unsafe { ffi::msg_usize(event, ffi::sel(c"modifierFlags")) }
}

/// `-[NSEvent keyCode]`, which is an `unsigned short` and not an `NSUInteger`.
///
/// # Safety
///
/// `event` must be a live key or flags-changed `NSEvent`.
unsafe fn keycode(event: Id) -> u16 {
    // SAFETY: the signature written here is the method's; a wider read would
    // pick up whatever the register above it held.
    let send: unsafe extern "C" fn(Id, Sel) -> u16 = unsafe { ffi::msg_send() };
    unsafe { send(event, ffi::sel(c"keyCode")) }
}

/// `-[NSEvent locationInWindow]`, in AppKit's window space with Y **up**.
///
/// # Safety
///
/// `event` must be a live mouse `NSEvent`.
unsafe fn location(event: Id) -> NSPoint {
    // SAFETY: an `NSPoint` is 16 bytes and comes back in registers on both
    // ABIs, so this is `msg_send` and never `msg_send_stret`.
    let send: unsafe extern "C" fn(Id, Sel) -> NSPoint = unsafe { ffi::msg_send() };
    unsafe { send(event, ffi::sel(c"locationInWindow")) }
}

/// The first character of an event's `charactersIgnoringModifiers`.
///
/// Read **here**, in the callback, because the string belongs to an event that
/// will not outlive it — the one interpretation this backend performs before the
/// shell is in hand, and [`events`](super::events) says why it has to be.
///
/// `characterAtIndex:` rather than a `String`, so a keystroke costs no
/// allocation. A key whose symbol is outside the basic plane would be lost by
/// that; no keyboard has one, and the astral case that genuinely matters —
/// committed text — goes through `insertText:` where it is handled whole.
///
/// # Safety
///
/// `event` must be a live key `NSEvent`.
unsafe fn first_character(event: Id) -> Option<char> {
    // SAFETY: a key event answers this with an autoreleased `NSString` or nil.
    let string = unsafe { ffi::msg(event, ffi::sel(c"charactersIgnoringModifiers")) };
    if string.is_null() {
        return None;
    }
    // SAFETY: `length` on a live `NSString`.
    if unsafe { ffi::msg_usize(string, ffi::sel(c"length")) } == 0 {
        return None;
    }
    // SAFETY: index zero is in range for a string of non-zero length;
    // `characterAtIndex:` answers a `unichar`, which is a `u16`.
    let unit = unsafe {
        let send: unsafe extern "C" fn(Id, Sel, NSUInteger) -> u16 = ffi::msg_send();
        send(string, ffi::sel(c"characterAtIndex:"), 0)
    };
    char::from_u32(u32::from(unit))
}

/// An `NSString`'s contents, whether it arrived as one or inside an
/// `NSAttributedString`.
///
/// `insertText:` and `setMarkedText:` are both documented to hand over either,
/// and an input method really does use both — the attributed form carries the
/// underlining a pre-edit is drawn with.
///
/// # Safety
///
/// `string` must be a live `NSString` or `NSAttributedString`, or `nil`, with an
/// autorelease pool in scope.
unsafe fn plain_string(string: Id) -> Option<String> {
    if string.is_null() {
        return None;
    }
    let plain = match ffi::class(c"NSAttributedString") {
        // SAFETY: `isKindOfClass:` is on `NSObject` and takes a live class.
        Some(attributed) if unsafe { is_kind_of(string, attributed) } => {
            // SAFETY: `-[NSAttributedString string]` returns the backing
            // `NSString`, autoreleased into the caller's pool.
            unsafe { ffi::msg(string, ffi::sel(c"string")) }
        }
        _ => string,
    };
    // SAFETY: a live `NSString`, with a pool in scope.
    unsafe { ffi::string_from_nsstring(plain) }
}

/// `-[NSObject isKindOfClass:]`.
///
/// # Safety
///
/// `object` must be a live object and `class` a live class.
unsafe fn is_kind_of(object: Id, class: Class) -> bool {
    // SAFETY: on `NSObject`, so every object implements it with this signature.
    let send: unsafe extern "C" fn(Id, Sel, Class) -> ObjcBool = unsafe { ffi::msg_send() };
    unsafe { send(object, ffi::sel(c"isKindOfClass:"), class) != ffi::NO }
}

// ---------------------------------------------------------------------------
// Reaching the shell
// ---------------------------------------------------------------------------

/// The `NSWindow` a view is in, or null.
///
/// # Safety
///
/// `view` must be a live `NSView`.
unsafe fn window_of(view: Id) -> Id {
    // SAFETY: an `NSView` accessor that may answer nil for a view that is not
    // in a window.
    unsafe { ffi::msg(view, ffi::sel(c"window")) }
}

/// The delegate object a view's window routes its events through.
///
/// **This is the whole of the view-to-shell mapping**, and it is why the view
/// needs no registry entry of its own: `[window delegate]` is the object
/// [`app::Delegate`](super::app::Delegate) created and registered, and
/// `release_window` clears it before the window closes — so a callback that
/// arrives afterwards finds `nil` and records nothing, which is exactly what it
/// should do.
///
/// # Safety
///
/// `view` must be a live `NSView`.
unsafe fn delegate_of(view: Id) -> Id {
    // SAFETY: a live view; `window` may be nil, and messaging nil answers nil.
    let window = unsafe { window_of(view) };
    if window.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: an `NSWindow` accessor that may answer nil.
    unsafe { ffi::msg(window, ffi::sel(c"delegate")) }
}

/// Records what `make` builds, against the shell that owns this view's window.
///
/// # Safety
///
/// `view` must be a live `NSView`.
unsafe fn record(view: Id, make: impl FnOnce(usize) -> RawEvent) {
    // SAFETY: a live view.
    let window = unsafe { window_of(view) };
    if window.is_null() {
        return;
    }
    // SAFETY: as above.
    let delegate = unsafe { delegate_of(view) };
    app::record(delegate, make(window as usize));
}

// ---------------------------------------------------------------------------
// The responder methods
// ---------------------------------------------------------------------------

/// `acceptsFirstResponder` — **`YES`**, and without it no key event ever reaches
/// this view.
///
/// `NSView` answers `NO`, so `makeFirstResponder:` would silently refuse and the
/// window would stay first responder itself. See the [module docs](self).
///
/// # Safety
///
/// Called only by AppKit, on an instance of the class built below.
unsafe extern "C" fn accepts_first_responder(_view: Id, _cmd: Sel) -> ObjcBool {
    YES
}

/// `acceptsFirstMouse:` — **`YES`**.
///
/// The click that brings a background window to the front is otherwise swallowed
/// by the activation, so a player's first shot after alt-tabbing back does
/// nothing. Windows and X11 both deliver it.
///
/// # Safety
///
/// Called only by AppKit, on an instance of the class built below.
unsafe extern "C" fn accepts_first_mouse(_view: Id, _cmd: Sel, _event: Id) -> ObjcBool {
    YES
}

/// `keyDown:` — the key edge, and then the text.
///
/// In that order, so the consumer sees the keystroke before whatever it
/// committed. The event's timestamp is stashed first, because `insertText:` is
/// called from inside `interpretKeyEvents:` and carries no event of its own.
///
/// # Safety
///
/// Called only by AppKit, on an instance of the class built below.
unsafe extern "C" fn key_down(view: Id, _cmd: Sel, event: Id) {
    // SAFETY: AppKit passes a live key `NSEvent`, and the view is live.
    unsafe {
        let stamp = seconds(event);
        record(view, |window| RawEvent::Key {
            window,
            keycode: keycode(event),
            character: first_character(event),
            flags: flags(event),
            state: ButtonState::Pressed,
            repeat: ffi::msg_bool(event, ffi::sel(c"isARepeat")),
            seconds: stamp,
        });
        app::with_shared(delegate_of(view), |shared| shared.set_keying(stamp));
        interpret(view, event);
    }
}

/// Hands a key event to the input method.
///
/// `interpretKeyEvents:` takes an array, because an application that batched its
/// own event loop could hand over several; this one always has exactly one.
///
/// # Safety
///
/// `view` must be a live `NSView` in a window, `event` a live key `NSEvent`, and
/// an autorelease pool must be in scope — which it is, because this runs inside
/// [`pump`](crate::Shell::pump).
unsafe fn interpret(view: Id, event: Id) {
    let Some(array_class) = ffi::class(c"NSArray") else {
        return;
    };
    // SAFETY: `arrayWithObject:` returns an autoreleased array holding the
    // event, and `interpretKeyEvents:` takes exactly that.
    unsafe {
        let array = ffi::msg1(array_class, ffi::sel(c"arrayWithObject:"), event);
        ffi::msg1_void(view, ffi::sel(c"interpretKeyEvents:"), array);
    }
}

/// `keyUp:`.
///
/// # Safety
///
/// Called only by AppKit, on an instance of the class built below.
unsafe extern "C" fn key_up(view: Id, _cmd: Sel, event: Id) {
    // SAFETY: AppKit passes a live key `NSEvent`.
    unsafe {
        record(view, |window| RawEvent::Key {
            window,
            keycode: keycode(event),
            character: first_character(event),
            flags: flags(event),
            state: ButtonState::Released,
            repeat: false,
            seconds: seconds(event),
        });
    }
}

/// `flagsChanged:` — **the only notification a modifier key produces.**
///
/// # Safety
///
/// Called only by AppKit, on an instance of the class built below.
unsafe extern "C" fn flags_changed(view: Id, _cmd: Sel, event: Id) {
    // SAFETY: AppKit passes a live `NSEvent`.
    unsafe {
        record(view, |window| RawEvent::FlagsChanged {
            window,
            keycode: keycode(event),
            flags: flags(event),
            seconds: seconds(event),
        });
    }
}

/// `mouseMoved:` and the three `*MouseDragged:` methods, which are one fact.
///
/// **Dragging is not moving, to AppKit.** While any button is held the system
/// stops sending `mouseMoved:` and sends `mouseDragged:`, `rightMouseDragged:`
/// or `otherMouseDragged:` instead — so a backend that implements only the first
/// loses every sample of every drag, which is the whole of a click-and-drag
/// gesture.
///
/// # Safety
///
/// Called only by AppKit, on an instance of the class built below.
unsafe extern "C" fn mouse_moved(view: Id, _cmd: Sel, event: Id) {
    // SAFETY: AppKit passes a live mouse `NSEvent`.
    unsafe {
        record(view, |window| RawEvent::PointerMotion {
            window,
            location: location(event),
            dx: ffi::msg_f64(event, ffi::sel(c"deltaX")),
            dy: ffi::msg_f64(event, ffi::sel(c"deltaY")),
            seconds: seconds(event),
        });
    }
}

/// `scrollWheel:`.
///
/// # Safety
///
/// Called only by AppKit, on an instance of the class built below.
unsafe extern "C" fn scroll_wheel(view: Id, _cmd: Sel, event: Id) {
    // SAFETY: AppKit passes a live scroll `NSEvent`.
    unsafe {
        record(view, |window| RawEvent::Scroll {
            window,
            dx: ffi::msg_f64(event, ffi::sel(c"scrollingDeltaX")),
            dy: ffi::msg_f64(event, ffi::sel(c"scrollingDeltaY")),
            precise: ffi::msg_bool(event, ffi::sel(c"hasPreciseScrollingDeltas")),
            location: location(event),
            flags: flags(event),
            seconds: seconds(event),
        });
    }
}

/// The six button methods' shared body.
///
/// # Safety
///
/// `view` must be live and `event` a live mouse `NSEvent`.
unsafe fn button(view: Id, event: Id, state: ButtonState) {
    // SAFETY: `buttonNumber` is an `NSInteger`; a live mouse event answers it.
    unsafe {
        let send: unsafe extern "C" fn(Id, Sel) -> isize = ffi::msg_send();
        let number = send(event, ffi::sel(c"buttonNumber"));
        record(view, |window| RawEvent::Button {
            window,
            number,
            state,
            location: location(event),
            flags: flags(event),
            seconds: seconds(event),
        });
    }
}

/// `mouseDown:`, `rightMouseDown:` and `otherMouseDown:`.
///
/// One implementation for the three, because `buttonNumber` already says which —
/// unlike Win32, where the thumb pair share a message and are told apart by the
/// high word of `wParam`.
///
/// # Safety
///
/// Called only by AppKit, on an instance of the class built below.
unsafe extern "C" fn mouse_down(view: Id, _cmd: Sel, event: Id) {
    // SAFETY: AppKit passes a live mouse `NSEvent`.
    unsafe { button(view, event, ButtonState::Pressed) };
}

/// `mouseUp:`, `rightMouseUp:` and `otherMouseUp:`.
///
/// # Safety
///
/// Called only by AppKit, on an instance of the class built below.
unsafe extern "C" fn mouse_up(view: Id, _cmd: Sel, event: Id) {
    // SAFETY: AppKit passes a live mouse `NSEvent`.
    unsafe { button(view, event, ButtonState::Released) };
}

/// `mouseEntered:`, from the tracking area.
///
/// # Safety
///
/// Called only by AppKit, on an instance of the class built below.
unsafe extern "C" fn mouse_entered(view: Id, _cmd: Sel, event: Id) {
    // SAFETY: AppKit passes a live `NSEvent` of the tracking type.
    unsafe {
        record(view, |window| RawEvent::PointerFocus {
            window,
            entered: true,
            location: location(event),
            seconds: seconds(event),
        });
    }
}

/// `mouseExited:`, from the tracking area.
///
/// # Safety
///
/// Called only by AppKit, on an instance of the class built below.
unsafe extern "C" fn mouse_exited(view: Id, _cmd: Sel, event: Id) {
    // SAFETY: AppKit passes a live `NSEvent` of the tracking type.
    unsafe {
        record(view, |window| RawEvent::PointerFocus {
            window,
            entered: false,
            location: location(event),
            seconds: seconds(event),
        });
    }
}

/// `cursorUpdate:` — **the only moment a cursor shape can be applied.**
///
/// AppKit re-establishes the cursor from the cursor rects and tracking areas
/// under the pointer on every movement, so a `[cursor set]` issued from
/// [`set_cursor`](crate::Shell::set_cursor) is overwritten before the next frame.
/// This is where it sticks, and it is why the shape a window wants is recorded
/// on [`Shared`](super::app::Shared) rather than applied when it is asked for.
///
/// A window that has never asked gets the arrow, explicitly. Leaving the cursor
/// alone instead would let whatever the pointer crossed on the way in persist
/// over our window.
///
/// # Safety
///
/// Called only by AppKit, on an instance of the class built below.
unsafe extern "C" fn cursor_update(view: Id, _cmd: Sel, _event: Id) {
    // SAFETY: a live view of a live window.
    let window = unsafe { window_of(view) };
    if window.is_null() {
        return;
    }
    // SAFETY: as above.
    let wanted =
        unsafe { app::with_shared(delegate_of(view), |s| s.cursor_of(window as usize)) }.flatten();
    // SAFETY: an `NSCursor` from a class method is a shared singleton AppKit
    // owns for the life of the process, so a recorded pointer is always live.
    unsafe {
        match wanted {
            Some(cursor) => ffi::msg_void(cursor as Id, ffi::sel(c"set")),
            None => {
                if let Some(class) = ffi::class(c"NSCursor") {
                    let arrow = ffi::msg(class, ffi::sel(c"arrowCursor"));
                    ffi::msg_void(arrow, ffi::sel(c"set"));
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// NSTextInputClient
// ---------------------------------------------------------------------------

/// `insertText:replacementRange:` — **the only place text exists.**
///
/// Called by the input method, from inside `interpretKeyEvents:`. The string is
/// filtered for control characters, because the input method really does commit
/// `\r` for Return and `\t` for Tab, and
/// [`TextCommit`](crate::ShellEvent::TextCommit) means *text* — the same filter
/// the other three backends apply. A commit that is entirely control characters
/// produces no event at all, which is what keeps
/// [`TextCommit::text`](crate::ShellEvent::TextCommit) non-empty.
///
/// # Safety
///
/// Called only by the input method, on an instance of the class built below.
unsafe extern "C" fn insert_text(view: Id, _cmd: Sel, string: Id, _replacement: NSRange) {
    // SAFETY: the input method passes an `NSString` or an `NSAttributedString`,
    // and the pool `pump` holds is in scope.
    let Some(text) = (unsafe { plain_string(string) }) else {
        return;
    };
    let text: String = text.chars().filter(|c| keys::is_text(*c)).collect();
    if text.is_empty() {
        return;
    }
    // SAFETY: a live view.
    let window = unsafe { window_of(view) };
    if window.is_null() {
        return;
    }
    // SAFETY: as above.
    let delegate = unsafe { delegate_of(view) };
    let stamp = app::with_shared(delegate, |shared| {
        shared.push_text(window as usize, text);
        // The keystroke's own timestamp, stashed by `keyDown:` before it handed
        // the event over — not the moment the input method got round to it.
        shared.keying()
    });
    let Some(seconds) = stamp else { return };
    app::record(
        delegate,
        RawEvent::TextCommitted {
            window: window as usize,
            seconds,
        },
    );
}

/// `doCommandBySelector:` — **deliberately nothing.**
///
/// The default implementation beeps at any command it does not recognize, and
/// `interpretKeyEvents:` turns Escape, Tab, Return and every arrow key into one.
/// A game would beep once per keystroke. The key edges themselves are already
/// reported from `keyDown:`, so nothing is lost by refusing to act here.
///
/// # Safety
///
/// Called only by the input method, on an instance of the class built below.
unsafe extern "C" fn do_command_by_selector(_view: Id, _cmd: Sel, _selector: Sel) {}

/// `setMarkedText:selectedRange:replacementRange:` — the pre-edit string.
///
/// Recorded as a *length* and never shown: the seam has no pre-edit event, and
/// an input method cannot compose at all without something answering
/// `hasMarkedText` and `markedRange` consistently.
///
/// # Safety
///
/// Called only by the input method, on an instance of the class built below.
unsafe extern "C" fn set_marked_text(
    view: Id,
    _cmd: Sel,
    string: Id,
    _selected: NSRange,
    _replacement: NSRange,
) {
    // SAFETY: the input method passes an `NSString` or `NSAttributedString`.
    let units = unsafe { plain_string(string) }.map_or(0, |text| text.encode_utf16().count());
    // SAFETY: a live view.
    unsafe { app::with_shared(delegate_of(view), |shared| shared.set_marked(units)) };
}

/// `unmarkText` — the input method gave up on the composition.
///
/// # Safety
///
/// Called only by the input method, on an instance of the class built below.
unsafe extern "C" fn unmark_text(view: Id, _cmd: Sel) {
    // SAFETY: a live view.
    unsafe { app::with_shared(delegate_of(view), |shared| shared.set_marked(0)) };
}

/// `hasMarkedText`.
///
/// # Safety
///
/// Called only by the input method, on an instance of the class built below.
unsafe extern "C" fn has_marked_text(view: Id, _cmd: Sel) -> ObjcBool {
    // SAFETY: a live view.
    let marked = unsafe { app::with_shared(delegate_of(view), super::app::Shared::marked) };
    if marked.unwrap_or(0) > 0 {
        YES
    } else {
        ffi::NO
    }
}

/// `markedRange`.
///
/// # Safety
///
/// Called only by the input method, on an instance of the class built below.
unsafe extern "C" fn marked_range(view: Id, _cmd: Sel) -> NSRange {
    // SAFETY: a live view.
    let marked = unsafe { app::with_shared(delegate_of(view), super::app::Shared::marked) };
    match marked.unwrap_or(0) {
        0 => NSRange::NONE,
        units => NSRange::new(0, units),
    }
}

/// `selectedRange` — always empty.
///
/// This backend has no document to select in; the seam delivers committed text
/// and nothing else. `NSNotFound` is the documented way to say so, and a length
/// of zero at location zero would instead claim an empty selection at the start
/// of a document that does not exist.
///
/// # Safety
///
/// Called only by the input method, on an instance of the class built below.
unsafe extern "C" fn selected_range(_view: Id, _cmd: Sel) -> NSRange {
    NSRange::NONE
}

/// `attributedSubstringForProposedRange:actualRange:` — always `nil`.
///
/// There is no document to read back, and `nil` is what the protocol documents
/// for a client that cannot answer.
///
/// # Safety
///
/// Called only by the input method, on an instance of the class built below.
unsafe extern "C" fn attributed_substring(
    _view: Id,
    _cmd: Sel,
    _range: NSRange,
    _actual: *mut NSRange,
) -> Id {
    ptr::null_mut()
}

/// `validAttributesForMarkedText` — an empty array.
///
/// The attributes a client is willing to have applied to its pre-edit, and this
/// one draws no pre-edit at all.
///
/// # Safety
///
/// Called only by the input method, on an instance of the class built below.
unsafe extern "C" fn valid_attributes(_view: Id, _cmd: Sel) -> Id {
    let Some(class) = ffi::class(c"NSArray") else {
        return ptr::null_mut();
    };
    // SAFETY: `+[NSArray array]` returns an autoreleased empty array, which is
    // valid for the pool `pump` holds.
    unsafe { ffi::msg(class, ffi::sel(c"array")) }
}

/// `firstRectForCharacterRange:actualRange:` — where the candidate window goes.
///
/// The window's own origin in screen coordinates, because **the seam does not
/// model a caret**: nothing above this crate has told it where text is being
/// typed. A Japanese candidate list therefore appears at the bottom-left of the
/// window rather than under the cursor. That is a real limitation and it is
/// stated rather than hidden; closing it needs a seam that can say "the caret is
/// here", which is a decision above this crate.
///
/// # Safety
///
/// Called only by the input method, on an instance of the class built below.
unsafe extern "C" fn first_rect(
    view: Id,
    _cmd: Sel,
    _range: NSRange,
    _actual: *mut NSRange,
) -> NSRect {
    // SAFETY: a live view.
    let window = unsafe { window_of(view) };
    if window.is_null() {
        return NSRect::default();
    }
    // SAFETY: `convertRectToScreen:` takes and returns an `NSRect`, which is 32
    // bytes and therefore the one shape that needs the struct-return
    // trampoline on x86_64.
    unsafe {
        let convert: unsafe extern "C" fn(Id, Sel, NSRect) -> NSRect = ffi::msg_send_stret();
        convert(
            window,
            ffi::sel(c"convertRectToScreen:"),
            NSRect::new(0.0, 0.0, 0.0, 0.0),
        )
    }
}

/// `characterIndexForPoint:` — always zero.
///
/// Asked when the user clicks inside a pre-edit. There is no document to index
/// into, and zero is the only in-range answer.
///
/// # Safety
///
/// Called only by the input method, on an instance of the class built below.
unsafe extern "C" fn character_index_for_point(
    _view: Id,
    _cmd: Sel,
    _point: NSPoint,
) -> NSUInteger {
    0
}

// ---------------------------------------------------------------------------
// Building the class
// ---------------------------------------------------------------------------

/// An `IMP` from the shape most of these methods have: `(id, SEL, NSEvent *)`.
///
/// One transmute site for a dozen callbacks rather than a dozen of them. Each
/// still names its own encoding in the table below, which is the part a wrong
/// answer would be invisible in.
fn event_imp(implementation: unsafe extern "C" fn(Id, Sel, Id)) -> Imp {
    // SAFETY: casting a correctly-typed `extern "C"` function to the runtime's
    // opaque `IMP`, which is how every method is installed; the encoding beside
    // it in the table describes the real signature.
    unsafe { core::mem::transmute::<unsafe extern "C" fn(Id, Sel, Id), Imp>(implementation) }
}

/// An `IMP` from the `(id, SEL) -> BOOL` shape.
fn bool_imp(implementation: unsafe extern "C" fn(Id, Sel) -> ObjcBool) -> Imp {
    // SAFETY: as [`event_imp`].
    unsafe {
        core::mem::transmute::<unsafe extern "C" fn(Id, Sel) -> ObjcBool, Imp>(implementation)
    }
}

/// An `IMP` from the `(id, SEL) -> NSRange` shape.
fn range_imp(implementation: unsafe extern "C" fn(Id, Sel) -> NSRange) -> Imp {
    // SAFETY: as [`event_imp`].
    unsafe { core::mem::transmute::<unsafe extern "C" fn(Id, Sel) -> NSRange, Imp>(implementation) }
}

/// Builds — or returns — the `NSView` subclass.
///
/// Same shape and same reasoning as [`app`](super::app)'s delegate class and
/// [`window`](super::window)'s window class, including the already-registered
/// fallback: a null from `objc_allocateClassPair` means the same code is loaded
/// twice in one process, and the class that is already there is ours by name and
/// by method table.
///
/// # Errors
///
/// [`ShellError::Connect`](crate::ShellError::Connect) if `NSView` is not in this
/// image or the runtime refused a method.
pub(super) fn view_class() -> Result<Class, crate::ShellError> {
    let outcome = VIEW.get_or_init(|| {
        let Some(superclass) = ffi::class(c"NSView") else {
            return Err(
                "the Objective-C runtime has no NSView, so this image has no AppKit in it"
                    .to_string(),
            );
        };
        // SAFETY: `NSView` is a live class, the name is a NUL-terminated
        // literal, and no extra instance storage is asked for — the
        // view-to-shell mapping goes through `[[self window] delegate]`, see
        // `delegate_of`.
        let class = unsafe { ffi::objc_allocateClassPair(superclass, VIEW_CLASS.as_ptr(), 0) };
        if class.is_null() {
            let Some(existing) = ffi::class(VIEW_CLASS) else {
                return Err(
                    "objc_allocateClassPair refused CrcblView and no class of that name exists"
                        .to_string(),
                );
            };
            log::debug!("the {VIEW_CLASS:?} class was already registered in this process");
            return Ok(existing as usize);
        }
        install(class)?;
        // SAFETY: a complete class pair that has not been registered.
        unsafe { ffi::objc_registerClassPair(class) };
        Ok(class as usize)
    });
    match outcome {
        Ok(class) => Ok(*class as Class),
        Err(detail) => Err(crate::ShellError::Connect {
            backend: crate::ShellBackend::AppKit,
            detail: detail.clone(),
        }),
    }
}

/// Adds every method and the protocol conformance to an unregistered class.
///
/// Split from [`view_class`] because the table is long enough to bury the
/// registration it sits inside, and because the encodings are the part worth
/// reading on their own: the runtime reads each one to build an `NSInvocation`
/// if anything ever forwards the method, and a wrong return width there is a
/// wrong-sized read nothing in this crate would see and an input method might.
fn install(class: Class) -> Result<(), String> {
    let void_event = c"v@:@";
    let bool_nothing = std::ffi::CString::new(format!("{}@:", ffi::ENC_BOOL))
        .expect("the encoding characters contain no NUL");
    let bool_event = std::ffi::CString::new(format!("{}@:@", ffi::ENC_BOOL))
        .expect("the encoding characters contain no NUL");
    let range_nothing = std::ffi::CString::new(format!("{}@:", ffi::ENC_RANGE))
        .expect("the encoding characters contain no NUL");
    let insert = std::ffi::CString::new(format!("v@:@{}", ffi::ENC_RANGE))
        .expect("the encoding characters contain no NUL");
    let set_marked = std::ffi::CString::new(format!("v@:@{0}{0}", ffi::ENC_RANGE))
        .expect("the encoding characters contain no NUL");
    let substring = std::ffi::CString::new(format!("@@:{0}^{0}", ffi::ENC_RANGE))
        .expect("the encoding characters contain no NUL");
    let first_rect_enc =
        std::ffi::CString::new(format!("{}@:{1}^{1}", ffi::ENC_RECT, ffi::ENC_RANGE))
            .expect("the encoding characters contain no NUL");
    let index_for_point = std::ffi::CString::new(format!("Q@:{}", ffi::ENC_POINT))
        .expect("the encoding characters contain no NUL");

    // Every entry is `(selector, implementation, type encoding)`.
    let methods: [(&CStr, Imp, &CStr); 22] = [
        (
            c"acceptsFirstResponder",
            bool_imp(accepts_first_responder),
            bool_nothing.as_c_str(),
        ),
        (
            c"acceptsFirstMouse:",
            // SAFETY: a correctly-typed `extern "C"` function cast to the
            // runtime's opaque `IMP`, with the matching encoding beside it.
            unsafe {
                core::mem::transmute::<unsafe extern "C" fn(Id, Sel, Id) -> ObjcBool, Imp>(
                    accepts_first_mouse,
                )
            },
            bool_event.as_c_str(),
        ),
        (c"keyDown:", event_imp(key_down), void_event),
        (c"keyUp:", event_imp(key_up), void_event),
        (c"flagsChanged:", event_imp(flags_changed), void_event),
        // The four motion methods, because a held button turns `mouseMoved:`
        // into one of the three drags and a backend with only the first loses
        // every sample of every drag.
        (c"mouseMoved:", event_imp(mouse_moved), void_event),
        (c"mouseDragged:", event_imp(mouse_moved), void_event),
        (c"rightMouseDragged:", event_imp(mouse_moved), void_event),
        (c"otherMouseDragged:", event_imp(mouse_moved), void_event),
        // The six button methods, which `buttonNumber` already tells apart.
        (c"mouseDown:", event_imp(mouse_down), void_event),
        (c"mouseUp:", event_imp(mouse_up), void_event),
        (c"rightMouseDown:", event_imp(mouse_down), void_event),
        (c"rightMouseUp:", event_imp(mouse_up), void_event),
        (c"otherMouseDown:", event_imp(mouse_down), void_event),
        (c"otherMouseUp:", event_imp(mouse_up), void_event),
        (c"scrollWheel:", event_imp(scroll_wheel), void_event),
        (c"mouseEntered:", event_imp(mouse_entered), void_event),
        (c"mouseExited:", event_imp(mouse_exited), void_event),
        (c"cursorUpdate:", event_imp(cursor_update), void_event),
        (
            c"insertText:replacementRange:",
            // SAFETY: as above.
            unsafe {
                core::mem::transmute::<unsafe extern "C" fn(Id, Sel, Id, NSRange), Imp>(insert_text)
            },
            insert.as_c_str(),
        ),
        (
            c"doCommandBySelector:",
            // SAFETY: as above. A `SEL` argument encodes as `:`.
            unsafe {
                core::mem::transmute::<unsafe extern "C" fn(Id, Sel, Sel), Imp>(
                    do_command_by_selector,
                )
            },
            c"v@::",
        ),
        (
            c"setMarkedText:selectedRange:replacementRange:",
            // SAFETY: as above.
            unsafe {
                core::mem::transmute::<unsafe extern "C" fn(Id, Sel, Id, NSRange, NSRange), Imp>(
                    set_marked_text,
                )
            },
            set_marked.as_c_str(),
        ),
    ];
    for (name, imp, types) in methods {
        add(class, name, imp, types)?;
    }

    // The rest of `NSTextInputClient`, whose signatures are each their own
    // shape and so are added one at a time.
    add(
        class,
        c"unmarkText",
        // SAFETY: as above.
        unsafe { core::mem::transmute::<unsafe extern "C" fn(Id, Sel), Imp>(unmark_text) },
        c"v@:",
    )?;
    add(
        class,
        c"hasMarkedText",
        bool_imp(has_marked_text),
        bool_nothing.as_c_str(),
    )?;
    add(
        class,
        c"markedRange",
        range_imp(marked_range),
        range_nothing.as_c_str(),
    )?;
    add(
        class,
        c"selectedRange",
        range_imp(selected_range),
        range_nothing.as_c_str(),
    )?;
    add(
        class,
        c"attributedSubstringForProposedRange:actualRange:",
        // SAFETY: as above.
        unsafe {
            core::mem::transmute::<unsafe extern "C" fn(Id, Sel, NSRange, *mut NSRange) -> Id, Imp>(
                attributed_substring,
            )
        },
        substring.as_c_str(),
    )?;
    add(
        class,
        c"validAttributesForMarkedText",
        // SAFETY: as above.
        unsafe {
            core::mem::transmute::<unsafe extern "C" fn(Id, Sel) -> Id, Imp>(valid_attributes)
        },
        c"@@:",
    )?;
    add(
        class,
        c"firstRectForCharacterRange:actualRange:",
        // SAFETY: as above. `NSRect` is the one return here the x86_64 ABI
        // hands back through a hidden pointer, which is a property of the
        // function's own signature and needs nothing special at this end.
        unsafe {
            core::mem::transmute::<
                unsafe extern "C" fn(Id, Sel, NSRange, *mut NSRange) -> NSRect,
                Imp,
            >(first_rect)
        },
        first_rect_enc.as_c_str(),
    )?;
    add(
        class,
        c"characterIndexForPoint:",
        // SAFETY: as above.
        unsafe {
            core::mem::transmute::<unsafe extern "C" fn(Id, Sel, NSPoint) -> NSUInteger, Imp>(
                character_index_for_point,
            )
        },
        index_for_point.as_c_str(),
    )?;

    // **Conformance is what makes `inputContext` non-nil**, and therefore what
    // makes `interpretKeyEvents:` reach an input method at all. Unlike the
    // window delegate's protocol — where declaring it is politeness — this one
    // is load-bearing: `-[NSResponder inputContext]` answers nil for a responder
    // that does not claim `NSTextInputClient`, and a nil input context means no
    // text, ever, with nothing anywhere reporting why.
    // SAFETY: the protocol is a linker-level object that may legitimately be
    // absent, which `objc_getProtocol` reports as null rather than crashing on.
    let protocol = unsafe { ffi::objc_getProtocol(c"NSTextInputClient".as_ptr()) };
    if protocol.is_null() {
        return Err(
            "the Objective-C runtime has no NSTextInputClient protocol, so no input method \
             could reach this view and text input would be silently dead"
                .to_string(),
        );
    }
    // SAFETY: `class` is still unregistered and `protocol` is live.
    if unsafe { ffi::class_addProtocol(class, protocol) } == ffi::NO {
        return Err("class_addProtocol refused NSTextInputClient on CrcblView".to_string());
    }
    Ok(())
}

/// One `class_addMethod`, with the refusal named.
fn add(class: Class, name: &CStr, imp: Imp, types: &CStr) -> Result<(), String> {
    // SAFETY: `class` is the class pair being built and has not been registered
    // yet, which is when methods may be added; the implementation matches the
    // encoding beside it at the call site.
    if unsafe { ffi::class_addMethod(class, ffi::sel(name), imp, types.as_ptr()) } == ffi::NO {
        return Err(format!("class_addMethod refused {name:?} on CrcblView"));
    }
    Ok(())
}

/// Attaches the tracking area that makes enter, exit and cursor updates exist.
///
/// [`IN_VISIBLE_RECT`](ffi::tracking::IN_VISIBLE_RECT) is why this is called
/// once, at creation, and never again: AppKit keeps the area glued to the view's
/// visible rectangle, so a resize needs no re-registration. Win32's equivalent —
/// `TrackMouseEvent` — is a **one-shot** request that has to be re-armed after
/// every leave, which is the same fact costing more.
///
/// The rectangle passed in is ignored for that reason, and is zero to say so.
///
/// # Safety
///
/// `view` must be a live `NSView`, on the main thread, with an autorelease pool
/// in scope.
pub(super) unsafe fn add_tracking_area(view: Id) {
    let Some(class) = ffi::class(c"NSTrackingArea") else {
        log::warn!(
            "the Objective-C runtime has no NSTrackingArea; pointer enter and leave will not \
             be reported for this window"
        );
        return;
    };
    // SAFETY: `alloc` then the designated initialiser; the owner is the view,
    // which implements `mouseEntered:`, `mouseExited:` and `cursorUpdate:`, and
    // a nil `userInfo` is documented.
    unsafe {
        let allocated = ffi::msg(class, ffi::sel(c"alloc"));
        let init: unsafe extern "C" fn(Id, Sel, NSRect, NSUInteger, Id, Id) -> Id = ffi::msg_send();
        let area = init(
            allocated,
            ffi::sel(c"initWithRect:options:owner:userInfo:"),
            NSRect::new(0.0, 0.0, 0.0, 0.0),
            ffi::tracking::OPTIONS,
            view,
            ptr::null_mut(),
        );
        if area.is_null() {
            log::warn!("-[NSTrackingArea initWithRect:options:owner:userInfo:] returned nil");
            return;
        }
        ffi::msg1_void(view, ffi::sel(c"addTrackingArea:"), area);
        // The view retains it, so this releases the one `alloc` gave us.
        ffi::msg_void(area, ffi::sel(c"release"));
    }
}

/// What can be checked about this class without a window.
///
/// **The class is built by the same function the real path calls**, which is
/// what makes this more than a restatement of the table: `objc_allocateClassPair`
/// and `class_addMethod` are runtime calls with no AppKit in them, so a spawned
/// `libtest` thread may make them even though it may never own a window. Two
/// silent failure modes are reachable here and nowhere else:
///
/// * a method the runtime **refused** — a duplicate selector, a malformed
///   encoding — which `view_class` turns into an error nobody would otherwise
///   see until a keystroke went missing;
/// * a selector spelled wrong, which installs a method AppKit will never call
///   and leaves the real one inherited from `NSView`.
///
/// What it cannot check is whether an encoding *describes* its implementation.
/// The runtime reads those only when it forwards a method through an
/// `NSInvocation`, which nothing in this crate does and an input method might;
/// `docs/backlog.md` records it as a gap rather than pretending otherwise.
#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn the_view_class_installs_every_method_input_arrives_through() {
        let class = view_class().expect("CrcblView is built from NSObject-level runtime calls");

        // The responder half. A missing one here is a whole category of input
        // that silently does not exist — the four drag and motion methods most
        // of all, because `mouseMoved:` alone covers up its own absence for as
        // long as the player is holding a button.
        for selector in [
            c"acceptsFirstResponder",
            c"acceptsFirstMouse:",
            c"keyDown:",
            c"keyUp:",
            c"flagsChanged:",
            c"mouseMoved:",
            c"mouseDragged:",
            c"rightMouseDragged:",
            c"otherMouseDragged:",
            c"mouseDown:",
            c"mouseUp:",
            c"rightMouseDown:",
            c"rightMouseUp:",
            c"otherMouseDown:",
            c"otherMouseUp:",
            c"scrollWheel:",
            c"mouseEntered:",
            c"mouseExited:",
            c"cursorUpdate:",
        ] {
            // SAFETY: a class is an object, and every object answers
            // `instancesRespondToSelector:`.
            let installed = unsafe {
                let send: unsafe extern "C" fn(Id, Sel, Sel) -> ObjcBool = ffi::msg_send();
                send(
                    class,
                    ffi::sel(c"instancesRespondToSelector:"),
                    ffi::sel(selector),
                ) != ffi::NO
            };
            assert!(installed, "CrcblView does not implement {selector:?}");
        }
    }

    #[test]
    fn the_view_class_is_a_text_input_client_and_answers_every_required_method() {
        // Conformance is what makes `-[NSResponder inputContext]` non-nil, and a
        // nil input context means `interpretKeyEvents:` reaches no input method
        // and **no text is ever committed** — with nothing anywhere saying so.
        // That is this platform's `TranslateMessage`, and this is the closest a
        // window-less test can get to it.
        let class = view_class().expect("CrcblView is built");
        // SAFETY: the protocol is a linker-level object that may be absent,
        // which `objc_getProtocol` reports as null.
        let protocol = unsafe { ffi::objc_getProtocol(c"NSTextInputClient".as_ptr()) };
        assert!(!protocol.is_null(), "this AppKit has no NSTextInputClient");
        // SAFETY: a class is an object; `conformsToProtocol:` is on `NSObject`.
        let conforms = unsafe {
            let send: unsafe extern "C" fn(Id, Sel, Id) -> ObjcBool = ffi::msg_send();
            send(class, ffi::sel(c"conformsToProtocol:"), protocol) != ffi::NO
        };
        assert!(conforms, "CrcblView does not claim NSTextInputClient");

        for selector in [
            c"insertText:replacementRange:",
            c"doCommandBySelector:",
            c"setMarkedText:selectedRange:replacementRange:",
            c"unmarkText",
            c"selectedRange",
            c"markedRange",
            c"hasMarkedText",
            c"attributedSubstringForProposedRange:actualRange:",
            c"validAttributesForMarkedText",
            c"firstRectForCharacterRange:actualRange:",
            c"characterIndexForPoint:",
        ] {
            // SAFETY: as above.
            let installed = unsafe {
                let send: unsafe extern "C" fn(Id, Sel, Sel) -> ObjcBool = ffi::msg_send();
                send(
                    class,
                    ffi::sel(c"instancesRespondToSelector:"),
                    ffi::sel(selector),
                ) != ffi::NO
            };
            assert!(
                installed,
                "NSTextInputClient requires {selector:?} and CrcblView does not have it"
            );
        }
    }

    #[test]
    fn the_class_is_built_once_and_answered_again_afterwards() {
        // `objc_allocateClassPair` returns null for a name that already exists,
        // which is what a second copy of this code in one process looks like.
        // The `OnceLock` and the fallback are what make that a shared class
        // rather than a failure, and asking twice is how that is visible.
        let first = view_class().expect("built");
        let second = view_class().expect("built again");
        assert_eq!(first, second);
        assert_eq!(
            ffi::class(VIEW_CLASS).expect("registered"),
            first,
            "the registered class is the one this module hands out"
        );
    }
}
