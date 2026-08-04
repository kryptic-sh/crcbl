//! Hand-written Objective-C runtime FFI, and the C structures AppKit passes
//! across it.
//!
//! `docs/plan/15-windowing.md`'s macOS row, which reads in full: "hand-written
//! Objective-C runtime FFI (`objc_msgSend`) to AppKit". Every declaration below
//! is ours; there is no `objc2`, no `cocoa`, no `core-foundation`, and no
//! framework of any kind.
//!
//! # Decision: `#[link]`, not `dlopen` — the same argument `win32::ffi` makes
//!
//! [`x11::ffi`](crate::x11::ffi) and [`wayland::ffi`](crate::wayland::ffi) both
//! load their libraries with `dlopen`, because the registry in
//! [`backend`](crate::backend) is a fall-through list and an entry has to be
//! able to fail at *runtime*: a `DT_NEEDED` on `libwayland-client.so.0` kills
//! the process in `ld.so` before [`open`](crate::open) runs, on any machine that
//! does not have it — and a machine without libwayland is an ordinary machine.
//!
//! **That premise does not hold here either.** AppKit, Foundation, QuartzCore,
//! CoreGraphics and `libobjc` are the operating system: a macOS that lacks them
//! is not a macOS this engine failed to support, it is one that cannot run a
//! process. There is nothing for a runtime probe to discover, and
//! `dlopen`ing five system frameworks would buy exactly one thing — a fallible
//! code path with no failure. macOS has a single backend, so nothing falls
//! through to anything and the registry consequence that forces the Linux
//! answer is absent.
//!
//! # Decision: `objc_msgSend` is transmuted at every call, from one place
//!
//! **`objc_msgSend` has no variadic ABI on `aarch64-apple-darwin`.** Apple's
//! arm64 calling convention passes variadic arguments on the stack while normal
//! arguments go in registers, and `objc_msgSend` is not a variadic function at
//! all — it is a trampoline that must be *called with the exact signature of the
//! method it dispatches to*, so that every argument is already in the register
//! the implementation will read it from. Declaring it as
//! `fn(Id, Sel, ...) -> Id` and calling it with three arguments compiles, links,
//! runs, and silently hands the method whatever happened to be in the register
//! it looked in.
//!
//! So this module exports the symbol as an opaque address and one generic
//! function, [`msg_send`], that transmutes it to a caller-supplied function
//! pointer type. Every call site therefore **writes the method's signature
//! down**, which is the only form of this that a compiler can check anything
//! about. The named helpers below ([`msg`], [`msg_void`], …) are that generic
//! applied to the shapes used more than twice; anything else spells its
//! signature out at the call site.
//!
//! # Decision: large struct returns take a different symbol on x86_64
//!
//! An `NSRect` is four `CGFloat`s. The System V ABI returns a 32-byte structure
//! through a hidden pointer, and the Objective-C runtime has a *separate*
//! trampoline for that case — `objc_msgSend_stret` — because the hidden pointer
//! displaces the receiver from its usual register. AArch64 has an indirect
//! result register (`x8`) that is not one of the argument registers, so it needs
//! no such split and `objc_msgSend_stret` does not exist there at all.
//!
//! [`msg_send_stret`] is that difference, and it is implemented on **both**
//! sides rather than only on the one the CI runner uses. A `cfg` arm that exists
//! for Apple silicon and silently does the wrong thing on an Intel Mac is not
//! portability; it is a defect that reports as a pass.
//!
//! # What is deliberately not declared
//!
//! | Not used | What it would buy | Why not |
//! | --- | --- | --- |
//! | `toggleFullScreen:` and `NSWindowStyleMaskFullScreen` | Spaces fullscreen | `docs/plan/15-windowing.md` drops exclusive fullscreen and keeps two modes; Spaces fullscreen is a third, with its own animation, its own space and its own failure modes. Borderless here is a frameless window at screen size — see [`window`](super::window) |
//! | `NSPasteboard`, `registerForDraggedTypes:` | clipboard, drops | M3. [`ShellCaps`](crate::ShellCaps) is clear on every bit they would set |
//! | `IOHIDManager`, `IOHIDGetAccelerationWithKey` | genuinely unaccelerated pointer deltas | `NSEvent`'s `deltaX`/`deltaY` carry the system's pointer acceleration and macOS exposes no public way to take it off. Reaching past AppKit into IOKit for it is a slice of its own; [`caps`](super::AppKitShell::caps) states exactly what [`RAW_POINTER_MOTION`](crate::ShellCaps::RAW_POINTER_MOTION) does and does not mean here |
//! | any `_windowResize*Cursor` and `busyButClickableCursor` | diagonal-resize and busy cursors | **private selectors.** `NSCursor`'s public set has no diagonal resize and no wait cursor; [`pointer::cursor_selector`](super::pointer::cursor_selector) names each approximation rather than calling a method Apple does not publish |
//! | `NSAutoreleasePool` as an object | a scope for autoreleased returns | [`objc_autoreleasePoolPush`]/[`objc_autoreleasePoolPop`] are the same thing without a message send, and are the form that works under ARC and non-ARC alike |
//! | `CGDisplayModeGetRefreshRate`'s `CGDisplayModeGetRefreshRateRational` twin | an exact 60000/1001 | it does not exist; the `double` this one returns already expresses 59.94, which is more than Win32's integer hertz can do |
//! | `NSApplication`'s delegate slot | screen-parameter changes | taking it would displace a host application's own delegate. `NSNotificationCenter` is the polite form and is what [`app`](super::app) uses |

#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]

use core::ffi::c_void;

// Only the macOS half sends messages or builds strings; on every other host this
// module is compiled for its structures alone (see the `extern` gate below), and
// an unconditional import would be an unused one there.
#[cfg(target_os = "macos")]
use core::ffi::{CStr, c_char};

// Apple dropped every 32-bit target years before this backend was written, and
// `CGFloat` is `f32` there rather than `f64` — so every signature in this module
// would be wrong on one. Failing in the compiler is the honest answer; silently
// compiling arithmetic that is half the width the ABI expects is not.
#[cfg(all(target_vendor = "apple", not(target_pointer_width = "64")))]
compile_error!(
    "the AppKit backend assumes a 64-bit Apple target, where CGFloat is f64; a 32-bit one \
     needs every signature in appkit::ffi revisited"
);

// ---------------------------------------------------------------------------
// Scalar aliases
// ---------------------------------------------------------------------------

/// Any Objective-C object pointer: `id`, and also `Class`, which is one.
///
/// One alias rather than a newtype per class, for the reason the X11 backend
/// gives for its single `Cookie` and the Win32 backend for its single `Handle`:
/// they are the same thing to the runtime, and the type system cannot check
/// which class a pointer actually names. Where the distinction matters it is in
/// the parameter name.
pub type Id = *mut c_void;

/// A `Class`, which is an object too. Named apart for readability only.
pub type Class = *mut c_void;

/// A selector — an interned method name, `SEL`.
pub type Sel = *const c_void;

/// An `IMP`: the function pointer behind a method.
///
/// Declared with no arguments because [`class_addMethod`] takes it as an opaque
/// address and the *type encoding* string beside it is what describes the real
/// signature. Every implementation this backend installs is cast to this from a
/// correctly-typed `extern "C"` function.
pub type Imp = unsafe extern "C" fn();

/// `CGFloat`. `f64` on every 64-bit Apple target; see the `compile_error!`
/// above.
pub type CGFloat = f64;

/// `NSInteger`.
pub type NSInteger = isize;

/// `NSUInteger`.
pub type NSUInteger = usize;

/// `BOOL`, as **`i8` rather than `bool`**.
///
/// It is `signed char` on x86_64 and `_Bool` on arm64, and only the second has
/// exactly two valid bit patterns. Rust's `bool` has exactly two, so reading an
/// x86_64 `BOOL` return directly into one would be undefined behaviour the day
/// some method answered `2`. Everything crossing this boundary is an `i8` and is
/// compared against zero; [`YES`] and [`NO`] are what goes the other way.
pub type ObjcBool = i8;

/// `YES`.
pub const YES: ObjcBool = 1;
/// `NO`.
pub const NO: ObjcBool = 0;

/// The type encoding of `BOOL` in a method signature, which is not the same
/// character on the two architectures.
///
/// `signed char` on x86_64, `_Bool` on arm64. It is passed to
/// [`class_addMethod`], where the runtime reads it to build an `NSInvocation` if
/// anything ever forwards one of these methods — so a wrong character is a
/// wrong-sized read of a return value in a code path nothing here exercises and
/// AppKit might.
#[cfg(target_arch = "aarch64")]
pub const ENC_BOOL: &str = "B";
/// See the `aarch64` arm.
#[cfg(not(target_arch = "aarch64"))]
pub const ENC_BOOL: &str = "c";

// ---------------------------------------------------------------------------
// Structures
// ---------------------------------------------------------------------------

/// `NSPoint` / `CGPoint`.
///
/// **Y is up.** The origin of the screen coordinate space is the bottom-left of
/// the primary display, which is the opposite of every other backend in this
/// crate; [`geometry`](super::geometry) is where that is converted and is the
/// only place it should be.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NSPoint {
    /// Distance from the left edge.
    pub x: CGFloat,
    /// Distance from the **bottom** edge.
    pub y: CGFloat,
}

/// `NSSize` / `CGSize`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NSSize {
    /// Width.
    pub width: CGFloat,
    /// Height.
    pub height: CGFloat,
}

impl NSSize {
    /// A size of `width` × `height`.
    #[must_use]
    pub const fn new(width: CGFloat, height: CGFloat) -> Self {
        Self { width, height }
    }
}

/// `NSRect` / `CGRect` — an origin and a size, never two corners.
///
/// The opposite of Win32's `RECT`, which is two corners and never an origin and
/// a size. Getting that backwards is the classic mistake on each platform, in
/// opposite directions.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NSRect {
    /// Bottom-left corner.
    pub origin: NSPoint,
    /// Extent.
    pub size: NSSize,
}

/// `NSRange` — the location and length `NSTextInputClient` speaks in.
///
/// **UTF-16 code units, not bytes and not characters**, because that is what an
/// `NSString` is indexed by. Sixteen bytes of two integers, so both target ABIs
/// return it in registers and it never needs [`msg_send_stret`] — unlike
/// [`NSRect`], which is the only shape here that does.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NSRange {
    /// Index of the first unit.
    pub location: NSUInteger,
    /// How many units.
    pub length: NSUInteger,
}

impl NSRange {
    /// `NSNotFound`, which is `NSIntegerMax` and **not** `NSUIntegerMax`.
    ///
    /// Getting that wrong matters: an input method compares against the
    /// documented constant, and a range at `NSUIntegerMax` is simply a very
    /// large index it will try to use.
    pub const NOT_FOUND: NSUInteger = NSInteger::MAX as NSUInteger;

    /// The empty range an input method reads as "there is none".
    pub const NONE: Self = Self {
        location: Self::NOT_FOUND,
        length: 0,
    };

    /// A range of `length` units starting at `location`.
    #[must_use]
    pub const fn new(location: NSUInteger, length: NSUInteger) -> Self {
        Self { location, length }
    }
}

/// The Objective-C type encoding of an `NSRange` on a 64-bit target.
///
/// Two `NSUInteger`s, which encode as `Q`. Passed to [`class_addMethod`] for
/// every `NSTextInputClient` method that takes or returns one; a wrong encoding
/// is a wrong-width read in the forwarding path nothing here exercises and an
/// input method might.
pub const ENC_RANGE: &str = "{_NSRange=QQ}";

/// The type encoding of an `NSRect`, which is a `CGRect`.
pub const ENC_RECT: &str = "{CGRect={CGPoint=dd}{CGSize=dd}}";

/// The type encoding of an `NSPoint`, which is a `CGPoint`.
pub const ENC_POINT: &str = "{CGPoint=dd}";

impl NSRect {
    /// A rectangle at `(x, y)` of `width` × `height`.
    #[must_use]
    pub const fn new(x: CGFloat, y: CGFloat, width: CGFloat, height: CGFloat) -> Self {
        Self {
            origin: NSPoint { x, y },
            size: NSSize { width, height },
        }
    }

    /// The edge opposite the origin's `y` — the **top** in AppKit's Y-up space.
    #[must_use]
    pub const fn max_y(self) -> CGFloat {
        self.origin.y + self.size.height
    }
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// `NSWindowStyleMask` values.
pub mod style {
    use super::NSUInteger;

    /// `NSWindowStyleMaskBorderless` — no frame at all, which is what
    /// borderless mode is made of.
    pub const BORDERLESS: NSUInteger = 0;
    /// `NSWindowStyleMaskTitled`.
    pub const TITLED: NSUInteger = 1 << 0;
    /// `NSWindowStyleMaskClosable`.
    pub const CLOSABLE: NSUInteger = 1 << 1;
    /// `NSWindowStyleMaskMiniaturizable`.
    pub const MINIATURIZABLE: NSUInteger = 1 << 2;
    /// `NSWindowStyleMaskResizable`.
    pub const RESIZABLE: NSUInteger = 1 << 3;
}

/// Everything else that is a bare number.
pub mod value {
    use super::{NSInteger, NSUInteger};

    /// `NSBackingStoreBuffered`. The only surviving backing store; the other
    /// two were deprecated in 10.13 and are gone.
    pub const BACKING_BUFFERED: NSUInteger = 2;

    /// `NSApplicationActivationPolicyRegular` — a normal application with a
    /// Dock icon, a menu bar and the ability to become frontmost.
    ///
    /// See [`app`](crate::appkit::app) for why an unbundled process asks for
    /// this explicitly.
    pub const ACTIVATION_POLICY_REGULAR: NSInteger = 0;

    /// `NSEventMaskAny`.
    pub const EVENT_MASK_ANY: NSUInteger = NSUInteger::MAX;

    /// `NSApplicationPresentationDefault` — the menu bar and Dock behave
    /// normally.
    pub const PRESENTATION_DEFAULT: NSUInteger = 0;
    /// `NSApplicationPresentationAutoHideDock`.
    pub const PRESENTATION_AUTO_HIDE_DOCK: NSUInteger = 1 << 0;
    /// `NSApplicationPresentationAutoHideMenuBar`.
    ///
    /// **Never set on its own.** `-[NSApplication setPresentationOptions:]`
    /// raises `NSInvalidArgumentException` for a combination it considers
    /// invalid, and auto-hiding the menu bar without also hiding or auto-hiding
    /// the Dock is one — an Objective-C exception unwinding through a Rust frame
    /// is undefined behaviour, so this is a crash rather than an error return.
    /// [`PRESENTATION_BORDERLESS`] is the pair that is legal.
    pub const PRESENTATION_AUTO_HIDE_MENU_BAR: NSUInteger = 1 << 2;
    /// The presentation a borderless window asks for: both bars out of the way,
    /// in the combination the API accepts.
    pub const PRESENTATION_BORDERLESS: NSUInteger =
        PRESENTATION_AUTO_HIDE_DOCK | PRESENTATION_AUTO_HIDE_MENU_BAR;

    /// `NSEventTypeKeyUp`.
    ///
    /// The one event type this backend inspects by number rather than by which
    /// responder method AppKit called, and it is there for a defect: **while
    /// Command is held, AppKit does not deliver `keyUp:` to a view at all.** The
    /// pump routes such an event to the key window itself; see
    /// [`AppKitShell::drain_events`](super::AppKitShell).
    pub const EVENT_TYPE_KEY_UP: NSUInteger = 11;
}

/// `NSEventModifierFlags`, and the device-dependent bits underneath them.
///
/// # Two layers, and only the lower one says *which* Shift
///
/// The documented flags in the high half (`1 << 16` upwards) say that *a* Shift
/// is held. `flagsChanged:` is the only notification a modifier key produces on
/// this platform — there is no `keyDown:` for Shift — so a backend that wants to
/// report [`ShiftLeft`](crcbl_core::KeyCode::ShiftLeft) going down has to know
/// which one changed and whether it is now down. The **device-dependent** bits in
/// the low half carry exactly that, one per physical key, and they are what
/// [`keys::modifier_is_down`](super::keys::modifier_is_down) reads.
///
/// They come from `IOKit/hidsystem/IOLLEvent.h`'s `NX_DEVICE*KEYMASK` rather
/// than from AppKit's own header, which is why they are spelled out here with
/// their IOKit names beside them.
pub mod modifier {
    use super::NSUInteger;

    /// `NSEventModifierFlagCapsLock` — **latched**, not held.
    pub const CAPS_LOCK: NSUInteger = 1 << 16;
    /// `NSEventModifierFlagShift`.
    pub const SHIFT: NSUInteger = 1 << 17;
    /// `NSEventModifierFlagControl`.
    pub const CONTROL: NSUInteger = 1 << 18;
    /// `NSEventModifierFlagOption` — the key labelled Option or Alt.
    pub const OPTION: NSUInteger = 1 << 19;
    /// `NSEventModifierFlagCommand`.
    pub const COMMAND: NSUInteger = 1 << 20;

    // `NSEventModifierFlagNumericPad` (`1 << 21`) and
    // `NSEventModifierFlagFunction` (`1 << 23`) are deliberately **not** here.
    // Nothing reads them — the first is a key-class marker rather than the Num
    // Lock it is mistaken for, and the second is a key the seam has no name for
    // — and this module is audited by use, so a constant nobody reads would be a
    // declaration nobody had checked. [`keys::modifiers`](super::keys::modifiers)
    // is where the argument for dropping both lives, and its tests spell the two
    // bits out to assert that dropping them is what happens.

    /// `NX_DEVICELCTLKEYMASK`.
    pub const DEVICE_LEFT_CONTROL: NSUInteger = 0x0000_0001;
    /// `NX_DEVICELSHIFTKEYMASK`.
    pub const DEVICE_LEFT_SHIFT: NSUInteger = 0x0000_0002;
    /// `NX_DEVICERSHIFTKEYMASK`.
    pub const DEVICE_RIGHT_SHIFT: NSUInteger = 0x0000_0004;
    /// `NX_DEVICELCMDKEYMASK`.
    pub const DEVICE_LEFT_COMMAND: NSUInteger = 0x0000_0008;
    /// `NX_DEVICERCMDKEYMASK`.
    pub const DEVICE_RIGHT_COMMAND: NSUInteger = 0x0000_0010;
    /// `NX_DEVICELALTKEYMASK`.
    pub const DEVICE_LEFT_OPTION: NSUInteger = 0x0000_0020;
    /// `NX_DEVICERALTKEYMASK`.
    pub const DEVICE_RIGHT_OPTION: NSUInteger = 0x0000_0040;
    /// `NX_DEVICERCTLKEYMASK`, which is **not** beside its left twin.
    pub const DEVICE_RIGHT_CONTROL: NSUInteger = 0x0000_2000;
}

/// `NSTrackingAreaOptions`.
pub mod tracking {
    use super::NSUInteger;

    /// `NSTrackingMouseEnteredAndExited` — the owner gets `mouseEntered:` and
    /// `mouseExited:`.
    pub const MOUSE_ENTERED_AND_EXITED: NSUInteger = 0x01;
    /// `NSTrackingCursorUpdate` — the owner gets `cursorUpdate:`, which is where
    /// a cursor shape has to be applied for it to survive the next movement.
    pub const CURSOR_UPDATE: NSUInteger = 0x04;
    /// `NSTrackingActiveAlways` — deliver even while another application is
    /// frontmost, which is the behaviour X11's `EnterNotify` and Win32's
    /// `WM_MOUSELEAVE` already have.
    pub const ACTIVE_ALWAYS: NSUInteger = 0x80;
    /// `NSTrackingInVisibleRect` — AppKit keeps the area glued to the view's
    /// visible rectangle, so a resize needs no re-registration. The rectangle
    /// passed to the initialiser is ignored.
    pub const IN_VISIBLE_RECT: NSUInteger = 0x200;

    /// What this backend asks for: enter, exit, cursor updates, always, and
    /// self-maintaining.
    pub const OPTIONS: NSUInteger =
        MOUSE_ENTERED_AND_EXITED | CURSOR_UPDATE | ACTIVE_ALWAYS | IN_VISIBLE_RECT;
}

// ---------------------------------------------------------------------------
// The runtime and the frameworks
// ---------------------------------------------------------------------------

// Every `extern` block in this module is macOS-only, and that is not the usual
// "this code cannot work elsewhere" gate. `#[link(kind = "framework")]` is
// **rejected by rustc on a non-Apple target** rather than merely being useless
// there, so a declaration outside this gate would stop the Linux build of this
// crate from compiling at all — and the pure modules beside it
// ([`geometry`](super::geometry), [`events`](super::events)) exist precisely so
// that the arithmetic is exercised by `cargo test` on the machine this engine is
// developed on. The types and constants above are compiled everywhere, which is
// what lets those modules name them.
#[cfg(target_os = "macos")]
mod sys {
    use super::{Class, Id, Imp, NSPoint, ObjcBool, Sel};
    use core::ffi::{c_char, c_void};

    // The Objective-C runtime itself: class and selector lookup, the dispatch
    // trampolines, and the runtime-defined class machinery this backend uses to
    // build its window delegate.
    #[link(name = "objc")]
    unsafe extern "C" {
        pub fn objc_getClass(name: *const c_char) -> Class;
        pub fn objc_getProtocol(name: *const c_char) -> Id;
        pub fn sel_registerName(name: *const c_char) -> Sel;

        /// The dispatch trampoline. **Never called through this declaration** —
        /// see [`msg_send`](super::msg_send) for why it is taken as an address
        /// and transmuted instead.
        pub fn objc_msgSend();

        /// The System V large-struct-return trampoline, which exists only where
        /// the ABI needs it. Not declared on `aarch64`, where the symbol is
        /// absent and a reference to it would fail to link.
        #[cfg(not(target_arch = "aarch64"))]
        pub fn objc_msgSend_stret();

        pub fn objc_allocateClassPair(
            superclass: Class,
            name: *const c_char,
            extra_bytes: usize,
        ) -> Class;
        pub fn objc_registerClassPair(class: Class);
        pub fn class_addMethod(class: Class, name: Sel, imp: Imp, types: *const c_char)
        -> ObjcBool;
        pub fn class_addProtocol(class: Class, protocol: Id) -> ObjcBool;

        pub fn objc_autoreleasePoolPush() -> *mut c_void;
        pub fn objc_autoreleasePoolPop(pool: *mut c_void);
    }

    // Foundation: `NSString`, `NSDate`, `NSNotificationCenter`, and the run-loop
    // mode the event pump names.
    #[link(name = "Foundation", kind = "framework")]
    unsafe extern "C" {
        /// `NSDefaultRunLoopMode`, an `NSString *`. Read once and passed to
        /// `nextEventMatchingMask:untilDate:inMode:dequeue:`.
        pub static NSDefaultRunLoopMode: Id;
    }

    // AppKit: every class this backend looks up by name lives here, and this is
    // the declaration that makes the framework — and therefore those classes —
    // part of the image.
    #[link(name = "AppKit", kind = "framework")]
    unsafe extern "C" {
        /// Posted when a display is attached, detached, or reconfigured. The
        /// name [`app`](crate::appkit::app) registers an observer for.
        pub static NSApplicationDidChangeScreenParametersNotification: Id;
    }

    // QuartzCore: `CAMetalLayer`, which is the whole of
    // [`SurfaceTarget::AppKit`](crcbl_core::SurfaceTarget::AppKit), and the
    // clock `NSEvent.timestamp` is measured on.
    //
    // `CAMetalLayer` itself is reached through `objc_getClass`, like every other
    // class — but the class only exists to be found if the framework is in the
    // image, and a `#[link]` attribute is what puts it there.
    #[link(name = "QuartzCore", kind = "framework")]
    unsafe extern "C" {
        /// Seconds since the system booted, on the same `mach_absolute_time`
        /// clock `-[NSEvent timestamp]` is measured against.
        ///
        /// That correspondence is the whole reason this is the function used
        /// rather than `-[NSProcessInfo systemUptime]`: rebasing a timestamp
        /// needs a reading of **the same** clock, and a second clock that is
        /// merely also called "uptime" would put every event a constant offset
        /// away from the truth. See [`TimeBase`](crate::appkit::TimeBase).
        pub fn CACurrentMediaTime() -> f64;
    }

    // CoreGraphics: the display-mode query, which is the only way to get a
    // refresh rate that can express 59.94 — and the pointer, which AppKit does
    // not own. Warping and disconnecting the mouse from the cursor are both
    // Quartz-level operations with no `NSCursor` equivalent.
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        pub fn CGMainDisplayID() -> u32;
        pub fn CGDisplayCopyDisplayMode(display: u32) -> *mut c_void;
        pub fn CGDisplayModeGetRefreshRate(mode: *mut c_void) -> f64;
        pub fn CGDisplayModeRelease(mode: *mut c_void);

        /// Moves the cursor, in **global display coordinates** — origin at the
        /// top-left of the main display, Y **down**, which is the seam's
        /// convention and the opposite of AppKit's.
        ///
        /// Returns a `CGError`; `kCGErrorSuccess` is zero.
        pub fn CGWarpMouseCursorPosition(position: NSPoint) -> i32;

        /// Connects or disconnects the hardware mouse from the cursor's
        /// position.
        ///
        /// Disconnected, the cursor stops moving and the deltas keep arriving —
        /// which is the whole of [`PointerMode::Locked`](crate::PointerMode) on
        /// this platform. `boolean_t` is `int` on every Darwin target.
        pub fn CGAssociateMouseAndMouseCursorPosition(connected: i32) -> i32;

        /// A `CGEventRef` describing the current input state, or null.
        ///
        /// **Declared for the test suite and used by nothing else**, which is
        /// why it carries a `cfg` no other declaration in this module needs:
        /// with a null source it is the cheapest way to ask where the cursor
        /// *is*, and it is Quartz rather than AppKit — so, unlike
        /// `+[NSEvent mouseLocation]`, a spawned test body may call it. That is
        /// what makes [`CGWarpMouseCursorPosition`] checkable as a mechanism
        /// without a window; see [`input`](super::super::input).
        #[cfg(test)]
        pub fn CGEventCreate(source: *mut c_void) -> *mut c_void;

        /// An event's location, in the same global display coordinates
        /// [`CGWarpMouseCursorPosition`] takes. See [`CGEventCreate`].
        #[cfg(test)]
        pub fn CGEventGetLocation(event: *mut c_void) -> NSPoint;
    }

    // CoreFoundation: one function, and it is here because `CGEventCreate`
    // returns a CF object and something has to give it back. Nothing else in
    // this backend is CF-typed, and nothing outside the test suite is CF at all.
    #[cfg(test)]
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        pub fn CFRelease(object: *mut c_void);
    }
}

#[cfg(target_os = "macos")]
pub use sys::{
    CACurrentMediaTime, CGAssociateMouseAndMouseCursorPosition, CGDisplayCopyDisplayMode,
    CGDisplayModeGetRefreshRate, CGDisplayModeRelease, CGMainDisplayID, CGWarpMouseCursorPosition,
    NSApplicationDidChangeScreenParametersNotification, NSDefaultRunLoopMode, class_addMethod,
    class_addProtocol, objc_allocateClassPair, objc_getProtocol, objc_registerClassPair,
};

/// The three declarations that exist for the macOS test suite alone.
#[cfg(all(target_os = "macos", test))]
pub use sys::{CFRelease, CGEventCreate, CGEventGetLocation};

// ---------------------------------------------------------------------------
// Class and selector lookup
// ---------------------------------------------------------------------------

/// The class named `name`, or `None` if this image does not have it.
///
/// `None` is not hypothetical paranoia: it is what a build that failed to link a
/// framework would produce, and turning it into a
/// [`ShellError::Connect`](crate::ShellError::Connect) naming the class is a
/// diagnosis. The alternative — sending a message to `nil` — does nothing and
/// returns zero, forever, silently.
#[cfg(target_os = "macos")]
#[must_use]
pub fn class(name: &CStr) -> Option<Class> {
    // SAFETY: `name` is a NUL-terminated C string that outlives the call; the
    // returned class is owned by the runtime and must not be freed.
    let class = unsafe { sys::objc_getClass(name.as_ptr()) };
    (!class.is_null()).then_some(class)
}

/// The selector for `name`, registering it if this process has not seen it.
///
/// Cannot fail: `sel_registerName` interns the string and returns a pointer into
/// the runtime's own table. Cheap enough to call per use — it is a hash lookup —
/// which is why there is no selector cache to keep in sync with the call sites.
#[cfg(target_os = "macos")]
#[must_use]
pub fn sel(name: &CStr) -> Sel {
    // SAFETY: `name` is a NUL-terminated C string that outlives the call.
    unsafe { sys::sel_registerName(name.as_ptr()) }
}

/// A scope in which autoreleased objects are valid, and at whose end they are
/// released.
///
/// Every AppKit method that returns an object it did not `alloc` — and that is
/// most of them, `[NSScreen screens]` and `[NSString stringWithUTF8String:]`
/// included — returns it **autoreleased**. With no pool in place the runtime
/// logs "autoreleased with no pool in place — just leaking" and does exactly
/// that. A normal Cocoa application gets a pool per turn of `NSApplication`'s
/// run loop; this backend does not have that loop, so it pushes its own around
/// every entry point that talks to AppKit.
///
/// [`objc_autoreleasePoolPush`] rather than an `NSAutoreleasePool` object: it is
/// the same mechanism with no message send, and it is the form that is legal
/// under ARC — which matters because a host application that embeds this engine
/// is compiled with ARC even though this crate is not.
#[cfg(target_os = "macos")]
#[derive(Debug)]
pub struct Pool(*mut c_void);

#[cfg(target_os = "macos")]
impl Pool {
    /// Pushes a pool. Popped by [`Drop`], in the reverse order of pushing,
    /// which the runtime requires and Rust's drop order gives for free.
    #[must_use]
    pub fn push() -> Self {
        // SAFETY: the runtime's own pool stack; the token is opaque and is
        // handed back unmodified in `drop`.
        Self(unsafe { sys::objc_autoreleasePoolPush() })
    }
}

#[cfg(target_os = "macos")]
impl Drop for Pool {
    fn drop(&mut self) {
        // SAFETY: `self.0` is the token this pool pushed and has not been popped
        // before. Rust's drop order guarantees the nesting the runtime wants.
        unsafe { sys::objc_autoreleasePoolPop(self.0) };
    }
}

// ---------------------------------------------------------------------------
// Message sending
// ---------------------------------------------------------------------------

/// `objc_msgSend`, transmuted to the exact signature of the method being sent.
///
/// **This is the one place the trampoline's address is taken**, and the reason
/// is the module documentation's: on `aarch64-apple-darwin` there is no variadic
/// ABI to declare it with, so the only correct call is one whose signature
/// matches the method's. Every caller therefore writes that signature down:
///
/// ```ignore
/// let set_frame: unsafe extern "C" fn(Id, Sel, NSRect, ObjcBool) =
///     unsafe { ffi::msg_send() };
/// unsafe { set_frame(window, ffi::sel(c"setFrame:display:"), rect, ffi::YES) };
/// ```
///
/// The size assertion is what stops `F` being anything but a function pointer —
/// `transmute_copy` would otherwise happily reinterpret a wider or narrower
/// type. It is a comparison of two constants and costs a shell call nothing.
///
/// # Safety
///
/// `F` must be a function pointer type whose signature is **exactly** the
/// receiver, the selector, and the arguments of the method that will be
/// dispatched, with that method's return type. A mismatch is not a type error at
/// any later point; it is arguments read out of the wrong registers.
#[cfg(target_os = "macos")]
#[inline]
#[must_use]
pub unsafe fn msg_send<F: Copy>() -> F {
    assert!(
        size_of::<F>() == size_of::<*const c_void>(),
        "msg_send's type parameter must be a function pointer"
    );
    // SAFETY: `F` is a function pointer of pointer size (asserted above), and
    // the value copied into it is the address of a real function. The caller
    // carries the obligation that the signature is the dispatched method's.
    unsafe { core::mem::transmute_copy(&(sys::objc_msgSend as *const c_void)) }
}

/// [`msg_send`] for a method whose return value does not fit in the ABI's return
/// registers.
///
/// On x86_64 that is `objc_msgSend_stret`, because the hidden return pointer
/// displaces the receiver out of `rdi`. On aarch64 the indirect result register
/// `x8` is not an argument register, nothing is displaced, and the symbol does
/// not exist — so the ordinary trampoline is correct and is what this returns.
///
/// In practice the only such type here is [`NSRect`], at 32 bytes. [`NSSize`]
/// and [`NSPoint`] are 16 and go back in registers on both architectures, which
/// is why `[screen frame]` needs this and `[window contentAspectRatio]` does
/// not.
///
/// # Safety
///
/// As [`msg_send`], and additionally: `F`'s return type must be one the target's
/// C ABI genuinely returns indirectly. Using this for a small return type is
/// wrong on x86_64 in the same way that not using it for a large one is.
#[cfg(target_os = "macos")]
#[inline]
#[must_use]
pub unsafe fn msg_send_stret<F: Copy>() -> F {
    assert!(
        size_of::<F>() == size_of::<*const c_void>(),
        "msg_send_stret's type parameter must be a function pointer"
    );
    #[cfg(target_arch = "aarch64")]
    let trampoline = sys::objc_msgSend as *const c_void;
    #[cfg(not(target_arch = "aarch64"))]
    let trampoline = sys::objc_msgSend_stret as *const c_void;
    // SAFETY: as `msg_send`, with the trampoline chosen for the target's
    // struct-return convention.
    unsafe { core::mem::transmute_copy(&trampoline) }
}

// The shapes used more than twice, so that the common call sites read as calls
// rather than as transmutes. Each is `msg_send` applied once, and each names its
// full signature in the same place its callers would have.
//
// `# Safety` on every one of these is the same sentence and is stated once here:
// the receiver must be a live object of a class that implements the selector,
// with arguments of the types the signature names. That is the obligation
// `objc_msgSend` always carries and none of these wrappers can discharge.

/// `[receiver sel]` returning an object.
///
/// # Safety
///
/// See the note above this function: `receiver` must implement `selector` with
/// this signature.
#[cfg(target_os = "macos")]
#[inline]
pub unsafe fn msg(receiver: Id, selector: Sel) -> Id {
    // SAFETY: the signature written here is the caller's obligation.
    let send: unsafe extern "C" fn(Id, Sel) -> Id = unsafe { msg_send() };
    unsafe { send(receiver, selector) }
}

/// `[receiver sel]` returning nothing.
///
/// # Safety
///
/// As [`msg`].
#[cfg(target_os = "macos")]
#[inline]
pub unsafe fn msg_void(receiver: Id, selector: Sel) {
    // SAFETY: the signature written here is the caller's obligation.
    let send: unsafe extern "C" fn(Id, Sel) = unsafe { msg_send() };
    unsafe { send(receiver, selector) };
}

/// `[receiver sel]` returning `BOOL`.
///
/// # Safety
///
/// As [`msg`].
#[cfg(target_os = "macos")]
#[inline]
pub unsafe fn msg_bool(receiver: Id, selector: Sel) -> bool {
    // SAFETY: the signature written here is the caller's obligation.
    let send: unsafe extern "C" fn(Id, Sel) -> ObjcBool = unsafe { msg_send() };
    unsafe { send(receiver, selector) != NO }
}

/// `[receiver sel]` returning a `CGFloat`.
///
/// # Safety
///
/// As [`msg`].
#[cfg(target_os = "macos")]
#[inline]
pub unsafe fn msg_f64(receiver: Id, selector: Sel) -> CGFloat {
    // SAFETY: the signature written here is the caller's obligation.
    let send: unsafe extern "C" fn(Id, Sel) -> CGFloat = unsafe { msg_send() };
    unsafe { send(receiver, selector) }
}

/// `[receiver sel]` returning an `NSUInteger`.
///
/// # Safety
///
/// As [`msg`].
#[cfg(target_os = "macos")]
#[inline]
pub unsafe fn msg_usize(receiver: Id, selector: Sel) -> NSUInteger {
    // SAFETY: the signature written here is the caller's obligation.
    let send: unsafe extern "C" fn(Id, Sel) -> NSUInteger = unsafe { msg_send() };
    unsafe { send(receiver, selector) }
}

/// `[receiver sel]` returning an `NSRect` — the one shape that needs
/// [`msg_send_stret`].
///
/// # Safety
///
/// As [`msg`].
#[cfg(target_os = "macos")]
#[inline]
pub unsafe fn msg_rect(receiver: Id, selector: Sel) -> NSRect {
    // SAFETY: `NSRect` is 32 bytes and is returned indirectly by both target
    // ABIs, which is exactly what `msg_send_stret` selects the trampoline for.
    let send: unsafe extern "C" fn(Id, Sel) -> NSRect = unsafe { msg_send_stret() };
    unsafe { send(receiver, selector) }
}

/// `[receiver sel:object]` returning an object.
///
/// # Safety
///
/// As [`msg`].
#[cfg(target_os = "macos")]
#[inline]
pub unsafe fn msg1(receiver: Id, selector: Sel, argument: Id) -> Id {
    // SAFETY: the signature written here is the caller's obligation.
    let send: unsafe extern "C" fn(Id, Sel, Id) -> Id = unsafe { msg_send() };
    unsafe { send(receiver, selector, argument) }
}

/// `[receiver sel:object]` returning nothing.
///
/// # Safety
///
/// As [`msg`].
#[cfg(target_os = "macos")]
#[inline]
pub unsafe fn msg1_void(receiver: Id, selector: Sel, argument: Id) {
    // SAFETY: the signature written here is the caller's obligation.
    let send: unsafe extern "C" fn(Id, Sel, Id) = unsafe { msg_send() };
    unsafe { send(receiver, selector, argument) };
}

/// `[receiver sel:BOOL]`.
///
/// # Safety
///
/// As [`msg`].
#[cfg(target_os = "macos")]
#[inline]
pub unsafe fn msg_set_bool(receiver: Id, selector: Sel, argument: bool) {
    // SAFETY: the signature written here is the caller's obligation.
    let send: unsafe extern "C" fn(Id, Sel, ObjcBool) = unsafe { msg_send() };
    unsafe { send(receiver, selector, if argument { YES } else { NO }) };
}

/// `[receiver sel:NSUInteger]`.
///
/// # Safety
///
/// As [`msg`].
#[cfg(target_os = "macos")]
#[inline]
pub unsafe fn msg_set_usize(receiver: Id, selector: Sel, argument: NSUInteger) {
    // SAFETY: the signature written here is the caller's obligation.
    let send: unsafe extern "C" fn(Id, Sel, NSUInteger) = unsafe { msg_send() };
    unsafe { send(receiver, selector, argument) };
}

/// `[receiver sel:NSSize]`.
///
/// # Safety
///
/// As [`msg`].
#[cfg(target_os = "macos")]
#[inline]
pub unsafe fn msg_set_size(receiver: Id, selector: Sel, argument: NSSize) {
    // SAFETY: the signature written here is the caller's obligation.
    let send: unsafe extern "C" fn(Id, Sel, NSSize) = unsafe { msg_send() };
    unsafe { send(receiver, selector, argument) };
}

/// `[receiver respondsToSelector:]`.
///
/// The version check this backend uses instead of a deployment-target macro:
/// `localizedName` arrived in 10.15 and `activate` in 14, and sending either to
/// a system that does not have it is a crash rather than a graceful nothing.
///
/// # Safety
///
/// `receiver` must be a live object; every object answers this selector.
#[cfg(target_os = "macos")]
#[inline]
pub unsafe fn responds_to(receiver: Id, selector: Sel) -> bool {
    // SAFETY: `respondsToSelector:` is on `NSObject`, so every object this
    // backend holds implements it with this signature.
    let send: unsafe extern "C" fn(Id, Sel, Sel) -> ObjcBool = unsafe { msg_send() };
    unsafe { send(receiver, sel(c"respondsToSelector:"), selector) != NO }
}

/// An `NSString` holding `text`, autoreleased into the innermost [`Pool`].
///
/// `None` when `text` contains an interior NUL, which would silently truncate
/// the string — the same refusal the Win32 backend makes for the same reason.
///
/// # Safety
///
/// An autorelease pool must be in scope, or the string leaks.
#[cfg(target_os = "macos")]
pub unsafe fn nsstring(text: &str) -> Option<Id> {
    let bytes = std::ffi::CString::new(text).ok()?;
    let class = class(c"NSString")?;
    // SAFETY: `stringWithUTF8String:` takes a NUL-terminated UTF-8 buffer and
    // copies it; `bytes` outlives the call.
    let send: unsafe extern "C" fn(Id, Sel, *const c_char) -> Id = unsafe { msg_send() };
    let string = unsafe { send(class, sel(c"stringWithUTF8String:"), bytes.as_ptr()) };
    (!string.is_null()).then_some(string)
}

/// An `NSString`'s contents, lossily.
///
/// Lossy for the reason the Win32 backend's device-name reader gives: this is a
/// display string — a monitor's name — and one bad byte is better shown as a
/// replacement character than dropped.
///
/// # Safety
///
/// `string` must be a live `NSString` (or `nil`, which answers `None`), and an
/// autorelease pool must be in scope.
#[cfg(target_os = "macos")]
pub unsafe fn string_from_nsstring(string: Id) -> Option<String> {
    if string.is_null() {
        return None;
    }
    // SAFETY: `UTF8String` returns a pointer into a buffer owned by the string
    // and valid until the innermost pool drains — which is after this call.
    let send: unsafe extern "C" fn(Id, Sel) -> *const c_char = unsafe { msg_send() };
    let utf8 = unsafe { send(string, sel(c"UTF8String")) };
    if utf8.is_null() {
        return None;
    }
    // SAFETY: `UTF8String` is documented to be NUL-terminated.
    Some(
        unsafe { CStr::from_ptr(utf8) }
            .to_string_lossy()
            .into_owned(),
    )
}
