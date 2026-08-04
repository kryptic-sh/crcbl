//! `NSApplication` bootstrap, the window delegate class, and what the delegate
//! records into.
//!
//! # Decision: the main thread is a precondition, not a preference
//!
//! AppKit is main-thread-only, and not in the soft sense that it is merely
//! recommended: `-[NSApplication nextEventMatchingMask:untilDate:inMode:dequeue:]`
//! carries an assertion that **raises `NSInternalInconsistencyException`** when
//! it is called from anywhere else, and an Objective-C exception unwinding
//! through a Rust frame is undefined behaviour. Even where a call does not
//! assert, the WindowServer's mach port is attached to the *main* run loop, so a
//! secondary thread's pump would create windows that never receive an event.
//!
//! So [`require_main_thread`] is checked in [`AppKitShell::open`](super::AppKitShell)
//! before anything else, and a shell created anywhere else is a clean
//! [`ShellError::Backend`] naming the rule. This is the concrete reason
//! [`Shell`](crate::Shell) is not `Send`, stated in the crate documentation and
//! enforced here.
//!
//! **This has a consequence for testing that is worth knowing before writing a
//! test rather than after.** Rust's `libtest` runs a test body on a thread it
//! spawns — measured on this workspace's toolchain, with and without
//! `--test-threads=1`, and true in both — so a `#[test]` can never be on the
//! main thread and can never drive a window. `docs/backlog.md` carries what that
//! costs and the one-line manifest change that would buy it back.
//!
//! # Decision: `NSApplicationActivationPolicyRegular`, explicitly
//!
//! A process that is not inside a `.app` bundle starts as a background
//! application: no Dock icon, no menu bar, and — the part that matters — it
//! **cannot become frontmost**, so its windows open behind everything and never
//! take the keyboard. `setActivationPolicy:` is the documented lever that turns
//! an unbundled binary into an ordinary application, and it is what every engine
//! that ships a plain executable calls.
//!
//! It is asked for rather than assumed, and its answer is kept: the call returns
//! `BOOL` and [`Bootstrap::policy_accepted`] is that answer, because "the window
//! opened but nothing could focus it" is a failure that otherwise arrives with
//! no diagnosis at all.
//!
//! # Decision: an observer, not the application delegate
//!
//! Screen reconfiguration is `applicationDidChangeScreenParameters:` on
//! `NSApplicationDelegate`, and taking that slot would displace the delegate of
//! any host application that embeds this engine — a shell is not entitled to be
//! the application. `NSNotificationCenter` delivers the same fact to an object
//! of our own, and the observer is removed when the shell is dropped.

use core::ffi::CStr;
use core::ptr;
use std::cell::{Cell, RefCell};
use std::sync::OnceLock;

use crate::{ShellBackend, ShellError};

use super::events::{RawEvent, enqueue};
use super::ffi::{self, Class, Id, Imp, ObjcBool, Pool, Sel, YES, value};

/// Everything the delegate and view callbacks write into, and the pump reads out
/// of.
///
/// The AppKit counterpart of the Win32 backend's `Shared`, and it exists for the
/// same reason: a callback runs re-entrantly inside a shell method, so it may
/// not touch the shell. A [`RefCell`] rather than a lock, because the whole of
/// this backend is main-thread-only and a second thread cannot reach it.
///
/// The three fields beside the queue are all there because a callback needs a
/// value the shell owns and cannot pass it one: an Objective-C object built by
/// [`objc_allocateClassPair`](super::ffi::objc_allocateClassPair) has no
/// instance storage here — see [`DELEGATES`] for why that is deliberate — so
/// anything a method needs across calls lives on this side of the boundary.
#[derive(Debug, Default)]
pub(super) struct Shared {
    queue: RefCell<Vec<RawEvent>>,
    /// Text the input method committed, waiting for its
    /// [`TextCommitted`](RawEvent::TextCommitted) marker to claim it.
    ///
    /// Keyed by window and consumed in order, which is the same arrangement the
    /// Win32 backend uses for a file drop's paths: the payload cannot live in a
    /// [`Copy`] enum, and its *place in the stream* still has to be kept.
    text: RefCell<Vec<(usize, String)>>,
    /// The timestamp of the key event currently inside `interpretKeyEvents:`.
    ///
    /// `insertText:` is called synchronously from within that call and carries
    /// no event of its own, so the commit would otherwise have to be stamped
    /// with the moment the input method got round to it. This is the keystroke's
    /// own timestamp, which is what the seam asks for.
    keying: Cell<f64>,
    /// How many UTF-16 units of pre-edit text the input method is holding.
    ///
    /// Not surfaced — the seam has no pre-edit event — but `hasMarkedText` and
    /// `markedRange` are required reading for an input method to compose at all,
    /// and both are answers about this number.
    marked: Cell<usize>,
    /// The `NSCursor` each window last asked for, as an integer.
    ///
    /// Read by `cursorUpdate:`, which is the only moment a cursor shape can be
    /// applied so that it survives the next pointer movement. The objects are
    /// AppKit's own shared singletons, so there is nothing to retain and nothing
    /// that can dangle.
    cursors: RefCell<Vec<(usize, usize)>>,
}

impl Shared {
    /// Records one thing a callback saw, collapsing it into a pending
    /// equivalent where [`enqueue`] says that is sound.
    ///
    /// Never panics on a borrow: nothing else holds this across a call into
    /// AppKit, which is the invariant that makes a `RefCell` here safe at all.
    fn push(&self, event: RawEvent) {
        enqueue(&mut self.queue.borrow_mut(), event);
    }

    /// Takes everything recorded so far.
    pub(super) fn take_events(&self) -> Vec<RawEvent> {
        core::mem::take(&mut self.queue.borrow_mut())
    }

    /// Queues a committed string for `window`.
    pub(super) fn push_text(&self, window: usize, text: String) {
        self.text.borrow_mut().push((window, text));
    }

    /// Takes the oldest string committed for `window`.
    ///
    /// In order, so that two commits in one pump reach the consumer as two
    /// [`TextCommit`](crate::ShellEvent::TextCommit)s in the order they were
    /// typed.
    pub(super) fn take_text(&self, window: usize) -> Option<String> {
        let mut queued = self.text.borrow_mut();
        let index = queued.iter().position(|(owner, _)| *owner == window)?;
        Some(queued.remove(index).1)
    }

    /// The timestamp of the key event being interpreted.
    pub(super) fn keying(&self) -> f64 {
        self.keying.get()
    }

    /// Records the timestamp of the key event about to be interpreted.
    pub(super) fn set_keying(&self, seconds: f64) {
        self.keying.set(seconds);
    }

    /// How much pre-edit text the input method is holding.
    pub(super) fn marked(&self) -> usize {
        self.marked.get()
    }

    /// Records how much pre-edit text the input method is holding.
    pub(super) fn set_marked(&self, units: usize) {
        self.marked.set(units);
    }

    /// The `NSCursor` a window wants, if it has asked for one.
    pub(super) fn cursor_of(&self, window: usize) -> Option<usize> {
        self.cursors
            .borrow()
            .iter()
            .find(|(owner, _)| *owner == window)
            .map(|(_, cursor)| *cursor)
    }

    /// Records the `NSCursor` a window wants.
    pub(super) fn set_cursor(&self, window: usize, cursor: usize) {
        let mut cursors = self.cursors.borrow_mut();
        match cursors.iter_mut().find(|(owner, _)| *owner == window) {
            Some(entry) => entry.1 = cursor,
            None => cursors.push((window, cursor)),
        }
    }

    /// Discards everything held for a window that has just been destroyed.
    ///
    /// The Win32 backend's `forget`, for the same reason: a callback that fired
    /// during `close` must not produce an event naming a handle the consumer has
    /// already been told is gone — [obligation 1](crate::Shell) forbids it. The
    /// text queue goes with it, or the next window to commit would be handed the
    /// dead one's string.
    pub(super) fn forget(&self, window: usize) {
        self.queue
            .borrow_mut()
            .retain(|event| event.window() != Some(window));
        self.text.borrow_mut().retain(|(owner, _)| *owner != window);
        self.cursors
            .borrow_mut()
            .retain(|(owner, _)| *owner != window);
    }
}

// ---------------------------------------------------------------------------
// The delegate → shell mapping
// ---------------------------------------------------------------------------

thread_local! {
    /// Which [`Shared`] each live delegate object belongs to.
    ///
    /// # Decision: a Rust-side table, not an Objective-C instance variable
    ///
    /// The obvious shape is the Win32 one — a pointer in the object, read back
    /// in the callback — and on this runtime that means `class_addIvar` plus
    /// `ivar_getOffset` plus pointer arithmetic into the middle of an
    /// Objective-C object. That is three chances to be wrong by a word, and
    /// being wrong by a word corrupts the object rather than failing; **none of
    /// it can be run, let alone falsified, from the machine this backend was
    /// written on.**
    ///
    /// A thread-local table is safe Rust with the same lifetime story and one
    /// extra lookup on a path that runs a handful of times per frame. It is
    /// thread-local rather than static because everything here is main-thread
    /// only, which makes the affinity the mechanism rather than a comment.
    ///
    /// The entries are raw pointers into [`Rc`](std::rc::Rc)-owned allocations
    /// the shell holds; [`Delegate::drop`] removes the entry before the shell
    /// drops the `Rc`, which is the whole soundness argument and is the same one
    /// `win32::shell`'s `GWLP_USERDATA` makes.
    static DELEGATES: RefCell<Vec<(usize, *const Shared)>> = const { RefCell::new(Vec::new()) };
}

/// Runs `read` against whichever [`Shared`] owns `delegate`.
///
/// A delegate this process does not know about answers `None` rather than being
/// an error: AppKit can deliver a notification queued before [`Delegate::drop`]
/// removed the entry, and there is nothing left to tell. A `nil` delegate — a
/// window whose delegate was cleared on the way out — is the same case and takes
/// the same path.
///
/// The table lookup finishes **before** `read` runs, so a callback that reaches
/// back into AppKit and re-enters this cannot deadlock on the borrow.
pub(super) fn with_shared<R>(delegate: Id, read: impl FnOnce(&Shared) -> R) -> Option<R> {
    let shared = DELEGATES.with_borrow(|table| {
        table
            .iter()
            .find(|(known, _)| *known == delegate as usize)
            .map(|(_, shared)| *shared)
    })?;
    // SAFETY: the pointer was registered by `Delegate::new` from an `Rc` the
    // shell owns, and `Delegate::drop` removes it before that `Rc` is dropped —
    // so a pointer still in the table names a live `Shared`. The borrow above
    // has already been released, so this cannot re-enter the table.
    Some(read(unsafe { &*shared }))
}

/// Records `event` against whichever shell owns `delegate`.
pub(super) fn record(delegate: Id, event: RawEvent) {
    with_shared(delegate, |shared| shared.push(event));
}

/// The `NSWindow *` a delegate notification is about, as the integer the queue
/// matches on.
///
/// # Safety
///
/// `notification` must be a live `NSNotification` or `nil`.
unsafe fn notified_window(notification: Id) -> usize {
    if notification.is_null() {
        return 0;
    }
    // SAFETY: every `NSNotification` answers `object`, and for the window
    // notifications this backend observes it is the `NSWindow`.
    unsafe { ffi::msg(notification, ffi::sel(c"object")) as usize }
}

// The delegate methods. Each is installed by `delegate_class` with the type
// encoding beside it, each does the least it can, and none of them touches the
// shell — see `super::events` for the whole argument.

/// `windowShouldClose:` — **answers `NO`, always.**
///
/// The seam makes closing a *question*: a
/// [`CloseRequested`](crate::ShellEvent::CloseRequested) is delivered and the
/// window stays open until
/// [`reply_close_request`](crate::Shell::reply_close_request) says otherwise.
/// Returning `YES` here would let AppKit close the window before anyone could
/// ask about unsaved work, which is the same interception the Win32 backend
/// performs by not passing `WM_CLOSE` to `DefWindowProc`.
///
/// # Safety
///
/// Called only by AppKit, with a delegate this module created.
unsafe extern "C" fn window_should_close(delegate: Id, _cmd: Sel, sender: Id) -> ObjcBool {
    record(
        delegate,
        RawEvent::CloseRequested {
            window: sender as usize,
        },
    );
    ffi::NO
}

/// `windowDidResize:`.
///
/// # Safety
///
/// Called only by AppKit, with a delegate this module created.
unsafe extern "C" fn window_did_resize(delegate: Id, _cmd: Sel, notification: Id) {
    // SAFETY: AppKit passes a live `NSNotification`.
    let window = unsafe { notified_window(notification) };
    record(delegate, RawEvent::Resized { window });
}

/// `windowDidChangeBackingProperties:` — a new `backingScaleFactor`, or a new
/// colour space.
///
/// # Safety
///
/// Called only by AppKit, with a delegate this module created.
unsafe extern "C" fn window_did_change_backing(delegate: Id, _cmd: Sel, notification: Id) {
    // SAFETY: AppKit passes a live `NSNotification`.
    let window = unsafe { notified_window(notification) };
    record(delegate, RawEvent::BackingChanged { window });
    // The scale change is published *before* the resize it comes with, and the
    // resize is what carries the new extent — so both are recorded, and
    // `translate` emits them in that order.
    record(delegate, RawEvent::Resized { window });
}

/// `windowDidChangeScreen:` — dragged onto another display.
///
/// # Safety
///
/// Called only by AppKit, with a delegate this module created.
unsafe extern "C" fn window_did_change_screen(delegate: Id, _cmd: Sel, notification: Id) {
    // SAFETY: AppKit passes a live `NSNotification`.
    let window = unsafe { notified_window(notification) };
    // The *monitor* in an effective `Borderless { monitor }` changed even when
    // the scale did not, and that is part of the configuration.
    record(delegate, RawEvent::BackingChanged { window });
}

/// `windowDidBecomeKey:`.
///
/// # Safety
///
/// Called only by AppKit, with a delegate this module created.
unsafe extern "C" fn window_did_become_key(delegate: Id, _cmd: Sel, notification: Id) {
    // SAFETY: AppKit passes a live `NSNotification`.
    let window = unsafe { notified_window(notification) };
    record(
        delegate,
        RawEvent::Focus {
            window,
            focused: true,
        },
    );
}

/// `windowDidResignKey:`.
///
/// # Safety
///
/// Called only by AppKit, with a delegate this module created.
unsafe extern "C" fn window_did_resign_key(delegate: Id, _cmd: Sel, notification: Id) {
    // SAFETY: AppKit passes a live `NSNotification`.
    let window = unsafe { notified_window(notification) };
    record(
        delegate,
        RawEvent::Focus {
            window,
            focused: false,
        },
    );
}

/// `windowWillClose:` — the window is going away without this shell asking.
///
/// # Safety
///
/// Called only by AppKit, with a delegate this module created.
unsafe extern "C" fn window_will_close(delegate: Id, _cmd: Sel, notification: Id) {
    // SAFETY: AppKit passes a live `NSNotification`.
    let window = unsafe { notified_window(notification) };
    record(delegate, RawEvent::Closed { window });
}

/// The screen-parameters observer. Named for this crate so it can never collide
/// with a real AppKit selector.
///
/// # Safety
///
/// Called only by `NSNotificationCenter`, with a delegate this module created.
unsafe extern "C" fn screens_changed(delegate: Id, _cmd: Sel, _notification: Id) {
    record(delegate, RawEvent::ScreensChanged);
}

/// The name of the class this module builds at runtime.
///
/// Not a per-process-unique name on purpose, exactly as the Win32 backend's
/// window class is not: two copies of this engine in one process should share
/// one class, and the [`OnceLock`] below is what makes that safe.
const DELEGATE_CLASS: &CStr = c"CrcblWindowDelegate";

/// The registration, once per process. `Err` carries what went wrong.
static DELEGATE: OnceLock<Result<usize, String>> = OnceLock::new();

/// Builds — or returns — the delegate class.
///
/// # Why it is built at runtime rather than declared
///
/// There is no Objective-C compiler in this build. `objc_allocateClassPair` plus
/// `class_addMethod` is what `@implementation` compiles *to*, and writing it out
/// is the whole of the "hand-written Objective-C runtime FFI" that
/// `docs/plan/15-windowing.md` asks for on this platform. The Win32 backend does
/// the same thing with `RegisterClassExW` and a `WNDPROC`; the difference is
/// only that Win32 has one entry point and this has one per message.
///
/// `objc_allocateClassPair` answering null means a class of that name already
/// exists — the same code loaded twice, in two modules — so the existing one is
/// looked up and used. It is ours by name and by method table, so delegating to
/// it is correct.
fn delegate_class() -> Result<Class, ShellError> {
    let outcome = DELEGATE.get_or_init(|| {
        let Some(superclass) = ffi::class(c"NSObject") else {
            return Err(
                "the Objective-C runtime has no NSObject, so this image has no \
                        Foundation in it"
                    .to_string(),
            );
        };
        // SAFETY: `NSObject` is a live class, the name is a NUL-terminated
        // literal, and no extra instance storage is asked for — the
        // delegate-to-shell mapping is a Rust table, see `DELEGATES`.
        let class = unsafe { ffi::objc_allocateClassPair(superclass, DELEGATE_CLASS.as_ptr(), 0) };
        if class.is_null() {
            // Already registered by another copy of this code in this process.
            let Some(existing) = ffi::class(DELEGATE_CLASS) else {
                return Err(
                    "objc_allocateClassPair refused CrcblWindowDelegate and no class \
                            of that name exists"
                        .to_string(),
                );
            };
            log::debug!("the {DELEGATE_CLASS:?} class was already registered in this process");
            return Ok(existing as usize);
        }

        let void_notification = c"v@:@";
        let bool_notification = std::ffi::CString::new(format!("{}@:@", ffi::ENC_BOOL))
            .expect("the encoding characters contain no NUL");
        // Every entry is `(selector, implementation, type encoding)`, and the
        // encoding has to match the implementation: the runtime reads it to
        // build an `NSInvocation` if AppKit ever forwards one of these, and a
        // wrong return width there is a wrong-sized read nothing here would see.
        let methods: [(&CStr, Imp, &CStr); 8] = [
            (
                c"windowShouldClose:",
                // SAFETY: casting a correctly-typed `extern "C"` function to the
                // runtime's opaque `IMP`, which is how every method is
                // installed; the encoding beside it describes the real
                // signature.
                unsafe {
                    core::mem::transmute::<unsafe extern "C" fn(Id, Sel, Id) -> ObjcBool, Imp>(
                        window_should_close,
                    )
                },
                bool_notification.as_c_str(),
            ),
            (
                c"windowDidResize:",
                // SAFETY: as above.
                unsafe {
                    core::mem::transmute::<unsafe extern "C" fn(Id, Sel, Id), Imp>(
                        window_did_resize,
                    )
                },
                void_notification,
            ),
            (
                c"windowDidChangeBackingProperties:",
                // SAFETY: as above.
                unsafe {
                    core::mem::transmute::<unsafe extern "C" fn(Id, Sel, Id), Imp>(
                        window_did_change_backing,
                    )
                },
                void_notification,
            ),
            (
                c"windowDidChangeScreen:",
                // SAFETY: as above.
                unsafe {
                    core::mem::transmute::<unsafe extern "C" fn(Id, Sel, Id), Imp>(
                        window_did_change_screen,
                    )
                },
                void_notification,
            ),
            (
                c"windowDidBecomeKey:",
                // SAFETY: as above.
                unsafe {
                    core::mem::transmute::<unsafe extern "C" fn(Id, Sel, Id), Imp>(
                        window_did_become_key,
                    )
                },
                void_notification,
            ),
            (
                c"windowDidResignKey:",
                // SAFETY: as above.
                unsafe {
                    core::mem::transmute::<unsafe extern "C" fn(Id, Sel, Id), Imp>(
                        window_did_resign_key,
                    )
                },
                void_notification,
            ),
            (
                c"windowWillClose:",
                // SAFETY: as above.
                unsafe {
                    core::mem::transmute::<unsafe extern "C" fn(Id, Sel, Id), Imp>(
                        window_will_close,
                    )
                },
                void_notification,
            ),
            (
                SCREENS_CHANGED,
                // SAFETY: as above.
                unsafe {
                    core::mem::transmute::<unsafe extern "C" fn(Id, Sel, Id), Imp>(screens_changed)
                },
                void_notification,
            ),
        ];
        for (name, imp, types) in methods {
            // SAFETY: `class` is the class pair this call is building and has
            // not been registered yet, which is when methods may be added; the
            // implementation matches the encoding beside it.
            let added = unsafe { ffi::class_addMethod(class, ffi::sel(name), imp, types.as_ptr()) };
            if added == ffi::NO {
                return Err(format!(
                    "class_addMethod refused {name:?} on CrcblWindowDelegate"
                ));
            }
        }

        // Declaring conformance is not what makes the methods reachable — AppKit
        // dispatches them through `respondsToSelector:` — but `setDelegate:` on
        // a modern AppKit is typed against the protocol, and a delegate that
        // does not claim it is the kind of thing a future release starts
        // refusing. Cheap, and the absence would be silent.
        // SAFETY: the protocol may legitimately be absent (it is a linker-level
        // object), which `class_addProtocol` reports rather than crashing on.
        let protocol = unsafe { ffi::objc_getProtocol(c"NSWindowDelegate".as_ptr()) };
        if !protocol.is_null() {
            // SAFETY: `class` is still unregistered and `protocol` is live.
            unsafe { ffi::class_addProtocol(class, protocol) };
        }

        // SAFETY: `class` is a complete class pair that has not been registered.
        unsafe { ffi::objc_registerClassPair(class) };
        Ok(class as usize)
    });
    match outcome {
        Ok(class) => Ok(*class as Class),
        Err(detail) => Err(ShellError::Connect {
            backend: ShellBackend::AppKit,
            detail: detail.clone(),
        }),
    }
}

/// The selector the screen-parameters observer is registered under.
const SCREENS_CHANGED: &CStr = c"crcblScreenParametersChanged:";

/// One shell's delegate object, and its observer registration.
///
/// Owns an Objective-C object, so it has a [`Drop`] that releases it — there is
/// no ARC here and nothing else will.
#[derive(Debug)]
pub(super) struct Delegate {
    object: Id,
}

impl Delegate {
    /// Creates the delegate and points it at `shared`.
    ///
    /// # Errors
    ///
    /// [`ShellError::Connect`] if the class could not be built or the object
    /// could not be allocated.
    pub(super) fn new(shared: *const Shared) -> Result<Self, ShellError> {
        let class = delegate_class()?;
        // SAFETY: `class` is a registered class; `alloc` then `init` is the
        // designated way to create an `NSObject`, and the result is owned by
        // this struct with a retain count of one.
        let object = unsafe {
            let allocated = ffi::msg(class, ffi::sel(c"alloc"));
            ffi::msg(allocated, ffi::sel(c"init"))
        };
        if object.is_null() {
            return Err(ShellError::Connect {
                backend: ShellBackend::AppKit,
                detail: "[[CrcblWindowDelegate alloc] init] returned nil".to_string(),
            });
        }
        DELEGATES.with_borrow_mut(|table| table.push((object as usize, shared)));

        // The screen-parameters observer. `object:nil` means "from any sender",
        // which is right — the notification comes from `NSApplication`.
        // SAFETY: `defaultCenter` is a class method on a live class; the
        // observer, the selector and the name are all live, and `nil` is a legal
        // sender filter. The centre keeps an **unretained** reference to the
        // observer, which is why `drop` below removes it before releasing.
        unsafe {
            if let Some(centre_class) = ffi::class(c"NSNotificationCenter") {
                let centre = ffi::msg(centre_class, ffi::sel(c"defaultCenter"));
                let add: unsafe extern "C" fn(Id, Sel, Id, Sel, Id, Id) = ffi::msg_send();
                add(
                    centre,
                    ffi::sel(c"addObserver:selector:name:object:"),
                    object,
                    ffi::sel(SCREENS_CHANGED),
                    ffi::NSApplicationDidChangeScreenParametersNotification,
                    ptr::null_mut(),
                );
            }
        }
        Ok(Self { object })
    }

    /// The object to hand `setDelegate:`.
    pub(super) const fn object(&self) -> Id {
        self.object
    }
}

impl Drop for Delegate {
    /// Deregisters the observer, forgets the shell mapping, and releases the
    /// object — in that order, and the order is the soundness argument.
    ///
    /// `NSNotificationCenter` holds an **unretained** reference to an observer,
    /// so an observer released while still registered leaves the centre with a
    /// dangling pointer it will message on the next display change. And a
    /// delegate whose mapping is still in [`DELEGATES`] could be handed a
    /// [`Shared`] the shell is about to drop; removing it first is what makes
    /// the raw pointer in that table sound.
    fn drop(&mut self) {
        // SAFETY: `self.object` is the live object this struct allocated.
        unsafe {
            if let Some(centre_class) = ffi::class(c"NSNotificationCenter") {
                let centre = ffi::msg(centre_class, ffi::sel(c"defaultCenter"));
                ffi::msg1_void(centre, ffi::sel(c"removeObserver:"), self.object);
            }
        }
        DELEGATES
            .with_borrow_mut(|table| table.retain(|(known, _)| *known != self.object as usize));
        // SAFETY: one `release` for the one retain `alloc` gave it. Nothing else
        // holds a reference: the windows' `delegate` slot is weak, and it has
        // been cleared by `AppKitShell::drop` before this runs.
        unsafe { ffi::msg_void(self.object, ffi::sel(c"release")) };
    }
}

// ---------------------------------------------------------------------------
// NSApplication
// ---------------------------------------------------------------------------

/// Whether this is the thread AppKit will accept calls on.
#[must_use]
pub(super) fn is_main_thread() -> bool {
    let Some(class) = ffi::class(c"NSThread") else {
        // No Foundation in the image at all. Everything after this will fail
        // anyway, and answering `false` makes it fail at the one place that
        // explains itself.
        return false;
    };
    // SAFETY: `isMainThread` is a class method on `NSThread` returning `BOOL`.
    unsafe { ffi::msg_bool(class, ffi::sel(c"isMainThread")) }
}

/// [`ShellError`] for a shell asked for on the wrong thread.
///
/// # Errors
///
/// Always, when this is not the main thread.
pub(super) fn require_main_thread() -> Result<(), ShellError> {
    if is_main_thread() {
        return Ok(());
    }
    Err(ShellError::Backend(
        "the AppKit backend must be opened on the process's main thread: AppKit asserts on it \
         and raises an Objective-C exception, and the WindowServer's event source is attached \
         to the main run loop, so a shell created anywhere else would either abort or open a \
         window that never receives an event"
            .to_string(),
    ))
}

/// What [`bootstrap`] did, kept so that a failure to focus has a diagnosis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Bootstrap {
    /// The `NSApplication` singleton.
    pub app: Id,
    /// Whether `setActivationPolicy:NSApplicationActivationPolicyRegular` was
    /// accepted.
    ///
    /// `false` means this process cannot become frontmost, so its windows will
    /// open behind everything and never take the keyboard. Not fatal — a window
    /// still exists and still renders — but it is the difference between a
    /// focus test that is meaningful and one that cannot pass, so it is recorded
    /// rather than discarded.
    pub policy_accepted: bool,
}

/// Whether the once-per-process half of [`bootstrap`] has run, and what it
/// found.
static LAUNCHED: OnceLock<bool> = OnceLock::new();

/// Brings `NSApplication` up far enough to own windows and deliver events.
///
/// The three steps, in the order AppKit requires:
///
/// 1. `sharedApplication`, which creates the singleton and — as a side effect
///    nothing else provides — connects the process to the WindowServer.
/// 2. `setActivationPolicy:`, before any window exists, so the first window
///    opens as a normal application's window rather than a background one's.
/// 3. `finishLaunching`, which posts `NSApplicationWillFinishLaunching` and
///    completes the launch AppKit would otherwise do inside `run` — and this
///    backend never calls `run`, because the seam's
///    [`pump`](crate::Shell::pump) *is* the loop.
///
/// Steps 2 and 3 are once per process; step 1 is idempotent by definition.
///
/// # Errors
///
/// [`ShellError::Connect`] if `NSApplication` is not in this image or the
/// singleton could not be created — which is what a process with no WindowServer
/// session looks like.
pub(super) fn bootstrap() -> Result<Bootstrap, ShellError> {
    let _pool = Pool::push();
    let Some(class) = ffi::class(c"NSApplication") else {
        return Err(ShellError::Connect {
            backend: ShellBackend::AppKit,
            detail: "the Objective-C runtime has no NSApplication, so this image has no AppKit \
                     in it"
                .to_string(),
        });
    };
    // SAFETY: `sharedApplication` is a class method returning the singleton,
    // autoreleased into the pool above but also retained by the runtime for the
    // life of the process.
    let app = unsafe { ffi::msg(class, ffi::sel(c"sharedApplication")) };
    if app.is_null() {
        return Err(ShellError::Connect {
            backend: ShellBackend::AppKit,
            detail: "[NSApplication sharedApplication] returned nil, which is what a process \
                     with no WindowServer session gets"
                .to_string(),
        });
    }

    let policy_accepted = *LAUNCHED.get_or_init(|| {
        // SAFETY: `setActivationPolicy:` takes an `NSInteger` and returns
        // `BOOL`; the value is the documented `Regular`.
        let accepted = unsafe {
            let set: unsafe extern "C" fn(Id, Sel, isize) -> ObjcBool = ffi::msg_send();
            set(
                app,
                ffi::sel(c"setActivationPolicy:"),
                value::ACTIVATION_POLICY_REGULAR,
            ) != ffi::NO
        };
        if !accepted {
            log::warn!(
                "setActivationPolicy:Regular was refused; this process cannot become \
                 frontmost, so its windows will open behind other applications and will not \
                 take the keyboard"
            );
        }
        // SAFETY: `finishLaunching` takes no arguments and returns nothing. It
        // is what `run` would have called, and this backend never calls `run`.
        unsafe { ffi::msg_void(app, ffi::sel(c"finishLaunching")) };
        accepted
    });

    activate(app);
    Ok(Bootstrap {
        app,
        policy_accepted,
    })
}

/// Brings this process to the front.
///
/// `activate` is the macOS 14 spelling and `activateIgnoringOtherApps:` the one
/// before it, which is deprecated rather than removed. Which exists is asked
/// with `respondsToSelector:` rather than assumed from a deployment target,
/// because sending a selector a system does not have is a crash and this crate
/// has no deployment target to consult.
fn activate(app: Id) {
    // SAFETY: `app` is the live `NSApplication` singleton; both selectors take
    // the arguments their branches pass and return nothing.
    unsafe {
        let modern = ffi::sel(c"activate");
        if ffi::responds_to(app, modern) {
            ffi::msg_void(app, modern);
        } else {
            let legacy: unsafe extern "C" fn(Id, Sel, ObjcBool) = ffi::msg_send();
            legacy(app, ffi::sel(c"activateIgnoringOtherApps:"), YES);
        }
    }
}
