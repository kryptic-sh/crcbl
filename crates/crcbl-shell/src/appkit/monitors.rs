//! `NSScreen` enumeration: geometry, visible frame, backing scale and the
//! refresh rate CoreGraphics will tell the truth about.
//!
//! # The global coordinate space is **points**, and no other backend's is
//!
//! Win32 lays every monitor out in one signed virtual screen of *pixels*, and
//! X11 in one screen of *pixels*. AppKit lays them out in one space of
//! **points**, with the origin at the bottom-left of the primary display —
//! which means the desktop is coherent in logical units and, on a desktop whose
//! displays run at different `backingScaleFactor`s, is not coherent in pixels at
//! all.
//!
//! That is the caveat [`MonitorInfo::bounds`] already states for Wayland,
//! arriving on a second platform for a different reason, and it decides two
//! things here:
//!
//! * [`MonitorInfo::bounds`] is each screen's own rectangle scaled by *its own*
//!   factor, so a 1× and a 2× display side by side do not tile. Use it to name a
//!   display and to tell a settings screen which one is on the left.
//! * **Window placement never goes through it.** A borderless window is put on a
//!   named screen by writing that screen's own point frame with `setFrame:`, so
//!   [`WINDOW_POSITION`](crate::ShellCaps::WINDOW_POSITION) is honest: the
//!   placement is exact, because it is expressed in the space AppKit actually
//!   has. [`Screen`] is what keeps those points around for it.
//!
//! # Decision: the identity is `CGDirectDisplayID`
//!
//! An `NSScreen *` is not an identity — AppKit hands out new objects for the
//! same display across a reconfiguration, and `[NSScreen screens]` is
//! re-created on every call. [`monitor`](crate::monitor) requires that a
//! [`MonitorId`] never be reused within a session, so the identity is the
//! display's `CGDirectDisplayID`, read out of `deviceDescription` under the
//! documented `NSScreenNumber` key. The Win32 backend maps `\\.\DISPLAYn` and
//! the X11 backend RandR CRTC ids through their own tables for exactly this
//! reason.
//!
//! # Decision: the refresh rate comes from CoreGraphics, and can say 59.94
//!
//! [`MonitorInfo::refresh_millihertz`] exists because 59.94 Hz is not 60, and
//! `docs/plan/15-windowing.md`'s frame pacing needs the difference. Two APIs
//! could answer here and only one of them can:
//!
//! * `-[NSScreen maximumFramesPerSecond]` is an `NSInteger` — whole hertz, the
//!   same limitation the Win32 backend documents and apologises for.
//! * `CGDisplayModeGetRefreshRate` is a `double`, and reports `59.94005…` for
//!   the mode in question.
//!
//! So this backend asks CoreGraphics. It answers `0.0` for a display whose rate
//! it will not state — a built-in panel with a variable rate, most often — and
//! zero is what [`MonitorInfo::refresh_millihertz`] documents as "the backend
//! cannot determine it" rather than as 0 Hz. **macOS is therefore the first of
//! the five backends that can report a non-integer refresh rate**, and the
//! millihertz unit stops being theoretical.

use crate::{MonitorId, MonitorInfo};

use super::ffi::{self, Id, NSRect, Sel};
use super::geometry::{self, Flip};
use super::shell::AppKitShell;

/// One screen, in the space AppKit actually uses.
///
/// Kept beside [`MonitorInfo`] rather than folded into it because the seam's
/// rectangle is in pixels and every placement call takes points. Converting back
/// would divide by a scale factor that the seam does not promise is the one the
/// rectangle was built with — see the module docs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Screen {
    /// The id the seam names this display by.
    pub id: MonitorId,
    /// `-[NSScreen frame]`, in points, Y up.
    pub frame: NSRect,
    /// `-[NSScreen visibleFrame]`, in points, Y up — the part not under the menu
    /// bar or the Dock.
    pub visible: NSRect,
    /// `-[NSScreen backingScaleFactor]`.
    pub scale: f64,
}

/// Everything `[NSScreen screens]` reports, in both forms.
///
/// # Safety
///
/// The main thread, with an autorelease pool in scope.
pub(super) unsafe fn enumerate(shell: &mut AppKitShell) -> (Vec<Screen>, Vec<MonitorInfo>) {
    let Some(class) = ffi::class(c"NSScreen") else {
        return (Vec::new(), Vec::new());
    };
    // SAFETY: `screens` is a class method returning an autoreleased `NSArray`.
    let array = unsafe { ffi::msg(class, ffi::sel(c"screens")) };
    if array.is_null() {
        return (Vec::new(), Vec::new());
    }
    // SAFETY: `count` on a live `NSArray`.
    let count = unsafe { ffi::msg_usize(array, ffi::sel(c"count")) };
    if count == 0 {
        // A session with no display attached. Not an error: the seam allows an
        // empty monitor list, and a headless Mac really has one.
        return (Vec::new(), Vec::new());
    }

    // `screens[0]` is documented as the screen with the menu bar, and its frame
    // origin is AppKit's own origin — so it is both "the primary display" and
    // the rectangle every Y flip is about. Reading it once, before the loop,
    // means every screen is flipped about the same edge.
    // SAFETY: index 0 is in range; `frame` returns an `NSRect`.
    let primary = unsafe {
        let object_at: unsafe extern "C" fn(Id, Sel, usize) -> Id = ffi::msg_send();
        let first = object_at(array, ffi::sel(c"objectAtIndex:"), 0);
        ffi::msg_rect(first, ffi::sel(c"frame"))
    };
    let flip = Flip::about_primary(primary);

    let mut screens = Vec::with_capacity(count);
    let mut monitors = Vec::with_capacity(count);
    for index in 0..count {
        // SAFETY: `index` is in range for the count just read, and the array is
        // still alive — nothing between here and the read releases the pool.
        let screen = unsafe {
            let object_at: unsafe extern "C" fn(Id, Sel, usize) -> Id = ffi::msg_send();
            object_at(array, ffi::sel(c"objectAtIndex:"), index)
        };
        if screen.is_null() {
            continue;
        }
        // SAFETY: all three are `NSScreen` accessors on a live screen.
        let (frame, visible, scale) = unsafe {
            (
                ffi::msg_rect(screen, ffi::sel(c"frame")),
                ffi::msg_rect(screen, ffi::sel(c"visibleFrame")),
                ffi::msg_f64(screen, ffi::sel(c"backingScaleFactor")),
            )
        };
        let scale = geometry::usable_scale(scale);
        // SAFETY: a live screen.
        let display = unsafe { display_id(screen) };
        let id = shell.monitor_id_for(display);
        // SAFETY: a live screen.
        let name = unsafe { name_of(screen) }
            .unwrap_or_else(|| format!("display {}", display.unwrap_or_default()));

        screens.push(Screen {
            id,
            frame,
            visible,
            scale,
        });
        monitors.push(MonitorInfo {
            id,
            name,
            bounds: geometry::physical_rect(flip.rect(frame), scale),
            // Already per screen and already in the same space as the frame,
            // unlike X11's single `_NET_WORKAREA`, so there is nothing to
            // intersect. It really is the visible area: AppKit takes the menu
            // bar and the Dock off it for us.
            work_area: geometry::physical_rect(flip.rect(visible), scale),
            scale_factor: scale,
            refresh_millihertz: display.map_or(0, refresh_millihertz),
            // Index zero is the menu-bar screen, which is what macOS means by
            // primary. Exactly one, always, which is the invariant
            // `MonitorInfo::is_primary` asks for.
            is_primary: index == 0,
        });
    }
    (screens, monitors)
}

/// A screen's `CGDirectDisplayID`, out of its device description.
///
/// `None` when the key is absent, which is what a screen that was detached
/// between `[NSScreen screens]` and this read answers. The caller falls back to
/// the main display's id rather than inventing one.
///
/// # Safety
///
/// `screen` must be a live `NSScreen`, with an autorelease pool in scope.
pub(super) unsafe fn display_id(screen: Id) -> Option<u32> {
    // SAFETY: `deviceDescription` is an `NSScreen` accessor returning an
    // autoreleased `NSDictionary`.
    let description = unsafe { ffi::msg(screen, ffi::sel(c"deviceDescription")) };
    if description.is_null() {
        return None;
    }
    // SAFETY: the key is a fresh autoreleased `NSString`; `objectForKey:` takes
    // one and returns an autoreleased object or nil.
    let number = unsafe {
        let key = ffi::nsstring("NSScreenNumber")?;
        ffi::msg1(description, ffi::sel(c"objectForKey:"), key)
    };
    if number.is_null() {
        return None;
    }
    // SAFETY: the value under `NSScreenNumber` is documented to be an
    // `NSNumber` wrapping a `CGDirectDisplayID`, which is a `uint32_t`.
    let send: unsafe extern "C" fn(Id, Sel) -> u32 = unsafe { ffi::msg_send() };
    Some(unsafe { send(number, ffi::sel(c"unsignedIntValue")) })
}

/// A screen's user-visible name.
///
/// `localizedName` arrived in macOS 10.15, so it is asked for with
/// `respondsToSelector:` rather than assumed — this crate has no deployment
/// target to consult, and sending a selector the system does not have is a
/// crash. `None` on an older system, where the caller falls back to the display
/// id; [`MonitorInfo::name`] documents itself as for display and logs only and
/// as never a key, so a numeric fallback costs nothing.
///
/// # Safety
///
/// `screen` must be a live `NSScreen`, with an autorelease pool in scope.
unsafe fn name_of(screen: Id) -> Option<String> {
    let selector = ffi::sel(c"localizedName");
    // SAFETY: a live object; `respondsToSelector:` is on `NSObject`.
    if !unsafe { ffi::responds_to(screen, selector) } {
        return None;
    }
    // SAFETY: the selector exists on this system and returns an autoreleased
    // `NSString`.
    let string = unsafe { ffi::msg(screen, selector) };
    // SAFETY: a live `NSString` or nil, both of which `string_from_nsstring`
    // accepts.
    unsafe { ffi::string_from_nsstring(string) }
}

/// A display's current refresh rate in millihertz, or zero if CoreGraphics will
/// not say.
///
/// See the module docs for why this is the CoreGraphics call and not
/// `maximumFramesPerSecond`. The `double` is multiplied and rounded rather than
/// truncated, so 59.94005994 becomes 59_940 rather than 59_940 by luck.
pub(super) fn refresh_millihertz(display: u32) -> u32 {
    // SAFETY: `CGDisplayCopyDisplayMode` takes a display id by value and returns
    // an owned mode or null. Nothing is dereferenced here.
    let mode = unsafe { ffi::CGDisplayCopyDisplayMode(display) };
    if mode.is_null() {
        return 0;
    }
    // SAFETY: `mode` is the owned mode just copied.
    let hertz = unsafe { ffi::CGDisplayModeGetRefreshRate(mode) };
    // SAFETY: the `Copy` in the name is the ownership rule; this is the matching
    // release, and `mode` is not used afterwards.
    unsafe { ffi::CGDisplayModeRelease(mode) };
    if !hertz.is_finite() || hertz <= 0.0 {
        // Documented for displays whose rate CoreGraphics will not state, which
        // includes most built-in panels. Zero is the seam's "cannot determine".
        return 0;
    }
    let millihertz = (hertz * 1000.0).round();
    if millihertz > f64::from(u32::MAX) {
        return u32::MAX;
    }
    millihertz as u32
}

/// The main display's id, for a screen whose own is unreadable.
///
/// Also the cheapest question there is about whether this process has a graphics
/// session at all — `0` is what a process with none gets — and it is thread-safe
/// CoreGraphics rather than main-thread-only AppKit, which is what lets the
/// suite ask it from a test body.
#[must_use]
pub(super) fn main_display() -> u32 {
    // SAFETY: takes nothing, returns a display id by value, and is documented as
    // thread-safe.
    unsafe { ffi::CGMainDisplayID() }
}

impl AppKitShell {
    /// The [`MonitorId`] for a display id, allocating one the first time.
    ///
    /// Never reuses an id within the session; see the [module docs](self). A
    /// screen whose own id could not be read borrows the main display's, which
    /// keeps it in the enumeration under a stable key rather than dropping it.
    pub(super) fn monitor_id_for(&mut self, display: Option<u32>) -> MonitorId {
        let display = display.unwrap_or_else(main_display);
        if let Some((_, id)) = self.monitor_ids.iter().find(|(known, _)| *known == display) {
            return *id;
        }
        let id = MonitorId(self.next_monitor_id);
        self.next_monitor_id += 1;
        self.monitor_ids.push((display, id));
        id
    }
}
