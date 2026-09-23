//! Hand-written Objective-C runtime FFI: the handful of runtime functions this
//! backend sends its messages through, and the GameController link.
//!
//! The same shape as `crcbl-shell`'s `appkit::ffi`, declared again here rather
//! than shared: that module is private to the shell, and this crate does not
//! depend on the shell. Only what the backend calls is declared.
//!
//! # `#[link]`, not `dlopen`
//!
//! The argument `appkit::ffi` makes for AppKit holds for GameController:
//! the framework ships with every macOS since 10.9, so a runtime probe would
//! add a failure path with no failure. `#[link(kind = "framework")]` is also
//! rejected by rustc on a non-Apple target, which is why this whole module is
//! macOS-only while the mapping beside it compiles everywhere.
//!
//! # `objc_msgSend` is transmuted at every call
//!
//! For `appkit::ffi`'s reason: on `aarch64-apple-darwin` the trampoline must be
//! called with the exact signature of the method it dispatches to, so it is
//! declared as an opaque symbol and [`msg_send`] transmutes it to a signature
//! each call site writes down. Every method sent here returns an object, a
//! `BOOL`, a `float` or an `NSUInteger`, all returned in registers on both
//! 64-bit Apple ABIs, so no `_stret` or `_fpret` variant is needed.

use core::ffi::{CStr, c_char, c_void};

/// Any Objective-C object pointer, `Class` included.
pub(super) type Id = *mut c_void;

/// A selector, `SEL`.
pub(super) type Sel = *const c_void;

/// `BOOL`, read as `i8` and compared against zero — `signed char` on x86_64,
/// where a `bool` could be handed a byte that is neither 0 nor 1.
type ObjcBool = i8;

/// `NSUInteger`.
pub(super) type NSUInteger = usize;

#[link(name = "objc")]
unsafe extern "C" {
    fn objc_getClass(name: *const c_char) -> Id;
    fn sel_registerName(name: *const c_char) -> Sel;

    /// The dispatch trampoline. **Never called through this declaration** —
    /// see [`msg_send`].
    fn objc_msgSend();

    fn objc_autoreleasePoolPush() -> *mut c_void;
    fn objc_autoreleasePoolPop(pool: *mut c_void);
}

// GameController: `GCController` and its profiles are reached through
// `objc_getClass`, and this is the declaration that puts the framework in the
// image for that lookup to find. Its statics are declared for the smoke test
// alone, which checks the category names `kind_of` matches against the
// framework's own constants.
#[link(name = "GameController", kind = "framework")]
unsafe extern "C" {
    #[cfg(test)]
    pub(super) static GCProductCategoryDualShock4: Id;
    #[cfg(test)]
    pub(super) static GCProductCategoryDualSense: Id;
    #[cfg(test)]
    pub(super) static GCProductCategoryXboxOne: Id;
    #[cfg(test)]
    pub(super) static GCProductCategorySwitchPro: Id;
    #[cfg(test)]
    pub(super) static GCProductCategorySwitchJoyConPair: Id;
    #[cfg(test)]
    pub(super) static GCProductCategoryMFi: Id;
}

/// The class named `name`, or `None` if this image does not have it.
pub(super) fn class(name: &CStr) -> Option<Id> {
    // SAFETY: `name` is NUL-terminated and outlives the call; the class is the
    // runtime's and is never freed.
    let class = unsafe { objc_getClass(name.as_ptr()) };
    (!class.is_null()).then_some(class)
}

/// The selector for `name`.
pub(super) fn sel(name: &CStr) -> Sel {
    // SAFETY: `name` is NUL-terminated and outlives the call.
    unsafe { sel_registerName(name.as_ptr()) }
}

/// A scope in which autoreleased returns are valid, released at its end —
/// every getter this backend sends returns its object autoreleased or
/// borrowed, and a poll outside AppKit's own loop has no pool of its own.
#[derive(Debug)]
pub(super) struct Pool(*mut c_void);

impl Pool {
    /// Pushes a pool, popped by `Drop` in reverse push order.
    #[must_use]
    pub(super) fn push() -> Self {
        // SAFETY: the runtime's own pool stack; the token goes back unchanged.
        Self(unsafe { objc_autoreleasePoolPush() })
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        // SAFETY: `self.0` is this pool's token, popped once, and Rust's drop
        // order gives the nesting the runtime requires.
        unsafe { objc_autoreleasePoolPop(self.0) };
    }
}

/// `objc_msgSend`, transmuted to the exact signature of the method sent.
///
/// # Safety
///
/// `F` must be a function pointer whose signature is exactly the receiver,
/// the selector, the arguments and the return type of the method dispatched.
#[inline]
#[must_use]
unsafe fn msg_send<F: Copy>() -> F {
    assert!(
        size_of::<F>() == size_of::<*const c_void>(),
        "msg_send's type parameter must be a function pointer"
    );
    // SAFETY: `F` is pointer-sized (asserted) and the value is the address of
    // a real function; the signature is the caller's obligation.
    unsafe { core::mem::transmute_copy(&(objc_msgSend as *const c_void)) }
}

// `# Safety` on each helper below is the one sentence `appkit::ffi` states
// once: the receiver must be a live object whose class implements the selector
// with the signature the helper names.

/// `[receiver selector]` returning an object.
///
/// # Safety
///
/// See the note above.
pub(super) unsafe fn msg(receiver: Id, selector: Sel) -> Id {
    // SAFETY: the signature written here is the caller's obligation.
    let send: unsafe extern "C" fn(Id, Sel) -> Id = unsafe { msg_send() };
    unsafe { send(receiver, selector) }
}

/// `[receiver selector]` returning nothing.
///
/// # Safety
///
/// As [`msg`].
pub(super) unsafe fn msg_void(receiver: Id, selector: Sel) {
    // SAFETY: the signature written here is the caller's obligation.
    let send: unsafe extern "C" fn(Id, Sel) = unsafe { msg_send() };
    unsafe { send(receiver, selector) };
}

/// `[receiver selector]` returning `BOOL`.
///
/// # Safety
///
/// As [`msg`].
pub(super) unsafe fn msg_bool(receiver: Id, selector: Sel) -> bool {
    // SAFETY: the signature written here is the caller's obligation.
    let send: unsafe extern "C" fn(Id, Sel) -> ObjcBool = unsafe { msg_send() };
    unsafe { send(receiver, selector) != 0 }
}

/// `[receiver selector]` returning a `float`.
///
/// # Safety
///
/// As [`msg`].
pub(super) unsafe fn msg_f32(receiver: Id, selector: Sel) -> f32 {
    // SAFETY: the signature written here is the caller's obligation.
    let send: unsafe extern "C" fn(Id, Sel) -> f32 = unsafe { msg_send() };
    unsafe { send(receiver, selector) }
}

/// `[receiver selector]` returning an `NSUInteger`.
///
/// # Safety
///
/// As [`msg`].
pub(super) unsafe fn msg_usize(receiver: Id, selector: Sel) -> NSUInteger {
    // SAFETY: the signature written here is the caller's obligation.
    let send: unsafe extern "C" fn(Id, Sel) -> NSUInteger = unsafe { msg_send() };
    unsafe { send(receiver, selector) }
}

/// `[array objectAtIndex:index]`.
///
/// # Safety
///
/// `array` must be a live `NSArray` with more than `index` elements.
pub(super) unsafe fn object_at(array: Id, index: NSUInteger) -> Id {
    // SAFETY: `objectAtIndex:` takes an `NSUInteger` and returns an object;
    // the bound is the caller's obligation.
    let send: unsafe extern "C" fn(Id, Sel, NSUInteger) -> Id = unsafe { msg_send() };
    unsafe { send(array, sel(c"objectAtIndex:"), index) }
}

/// `[receiver respondsToSelector:selector]` — how a getter newer than the
/// running macOS is told from one that is there.
///
/// # Safety
///
/// `receiver` must be a live object; every `NSObject` answers this.
pub(super) unsafe fn responds_to(receiver: Id, selector: Sel) -> bool {
    // SAFETY: `respondsToSelector:` is on `NSObject` with this signature.
    let send: unsafe extern "C" fn(Id, Sel, Sel) -> ObjcBool = unsafe { msg_send() };
    unsafe { send(receiver, sel(c"respondsToSelector:"), selector) != 0 }
}

/// `[object isKindOfClass:class]`. Test-only.
///
/// # Safety
///
/// `object` must be a live object and `class` a class.
#[cfg(test)]
pub(super) unsafe fn is_kind_of(object: Id, class: Id) -> bool {
    // SAFETY: `isKindOfClass:` is on `NSObject` with this signature.
    let send: unsafe extern "C" fn(Id, Sel, Id) -> ObjcBool = unsafe { msg_send() };
    unsafe { send(object, sel(c"isKindOfClass:"), class) != 0 }
}

/// `[class instancesRespondToSelector:selector]`. Test-only.
///
/// # Safety
///
/// `class` must be a class descending from `NSObject`.
#[cfg(test)]
pub(super) unsafe fn instances_respond_to(class: Id, selector: Sel) -> bool {
    // SAFETY: `+instancesRespondToSelector:` is on `NSObject` with this
    // signature.
    let send: unsafe extern "C" fn(Id, Sel, Sel) -> ObjcBool = unsafe { msg_send() };
    unsafe { send(class, sel(c"instancesRespondToSelector:"), selector) != 0 }
}

/// An `NSString`'s contents, lossily, or `None` for `nil`.
///
/// # Safety
///
/// `string` must be a live `NSString` or null, with a pool in scope.
pub(super) unsafe fn string(string: Id) -> Option<String> {
    if string.is_null() {
        return None;
    }
    // SAFETY: `UTF8String` returns a NUL-terminated buffer owned by the string
    // and valid until the innermost pool drains, after this call.
    let send: unsafe extern "C" fn(Id, Sel) -> *const c_char = unsafe { msg_send() };
    let utf8 = unsafe { send(string, sel(c"UTF8String")) };
    if utf8.is_null() {
        return None;
    }
    // SAFETY: as above.
    Some(
        unsafe { CStr::from_ptr(utf8) }
            .to_string_lossy()
            .into_owned(),
    )
}
