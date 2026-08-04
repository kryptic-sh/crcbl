//! The window class this backend builds at runtime, window creation, the
//! `CAMetalLayer`, and the borderless round trip.
//!
//! # Decision: an `NSWindow` subclass, because a borderless one cannot take the
//! keyboard
//!
//! `-[NSWindow canBecomeKeyWindow]` returns `NO` for a window whose style mask
//! has neither `NSWindowStyleMaskTitled` nor `NSWindowStyleMaskResizable` — and
//! `NSWindowStyleMaskBorderless` is zero, so a borderless window is exactly that
//! case. A game that went borderless would stop receiving keystrokes, with
//! nothing anywhere reporting why.
//!
//! Overriding it is the documented answer and needs a subclass, which on this
//! runtime means a second `objc_allocateClassPair` beside
//! [`app`](super::app)'s. `canBecomeMainWindow` is overridden with it: *main* is
//! what the menu bar acts on, and a borderless window that is key but not main
//! leaves the previous application's menus on screen.
//!
//! # Decision: borderless is a style swap, and the way back has to be exact
//!
//! `docs/plan/15-windowing.md` keeps two display modes and drops exclusive
//! fullscreen, and — separately — this backend does **not** use
//! `toggleFullScreen:`. Spaces fullscreen is a third mode with its own
//! animation, its own space, its own `NSWindowDelegate` callbacks and its own
//! ways to fail; what the plan asks for is a frameless window at screen size,
//! which is what this does.
//!
//! So: remember the style mask and the **frame** rectangle, drop to borderless,
//! and `setFrame:` the target screen's own rectangle. The way back restores both,
//! in that order — mask first, because `setFrame:` positions the window for the
//! mask it has and restoring the rectangle under a borderless mask would put a
//! titled window where a frameless one fitted. The desktop's display mode is
//! never touched, which is the whole reason the mode exists.
//!
//! Unlike the Win32 backend there is no `WINDOWPLACEMENT` to save: AppKit's
//! zoomed state is a property of the window rather than a rectangle to restore,
//! and `-[NSWindow setFrame:display:]` on a zoomed window un-zooms it in the way
//! the user's next click on the zoom button expects.
//!
//! # Decision: the shell owns the `CAMetalLayer`
//!
//! [`SurfaceTarget::AppKit`](crcbl_core::SurfaceTarget::AppKit) carries a
//! `CAMetalLayer *` and not an `NSView *`, and `crcbl_core::surface` says why in
//! as many words: Metal takes the layer directly and `VK_EXT_metal_surface`
//! takes it through MoltenVK, so **no HAL backend ever has to touch AppKit**.
//! That only works if somebody creates the layer, and that somebody is here.
//!
//! The view is made *layer-hosting* rather than layer-backed, and the two calls
//! go in the order `setLayer:` then `setWantsLayer:YES`. Backwards, AppKit
//! creates its own `CALayer` first and the `CAMetalLayer` set afterwards is
//! replaced — a window that renders nothing, with no error anywhere.

use core::ffi::CStr;
use core::ptr;
use std::sync::OnceLock;

use crate::{
    DisplayMode, MonitorId, PhysicalSize, ShellBackend, ShellError, SizeConstraints, WindowDesc,
};

use super::ffi::{self, Class, Id, Imp, NSRect, NSSize, ObjcBool, Sel, YES};
use super::geometry;
use super::monitors::Screen;
use super::shell::{AppKitShell, AppWindow};

/// The name of the `NSWindow` subclass this module builds at runtime.
const WINDOW_CLASS: &CStr = c"CrcblWindow";

/// The registration, once per process. `Err` carries what went wrong.
static WINDOW: OnceLock<Result<usize, String>> = OnceLock::new();

/// `canBecomeKeyWindow` / `canBecomeMainWindow` — **always `YES`**.
///
/// One implementation for both, because the answer and the signature are the
/// same and two copies would be two things to keep in step. See the
/// [module docs](self) for what returning `NO` costs a borderless window.
///
/// # Safety
///
/// Called only by AppKit, on an instance of the class built below.
unsafe extern "C" fn can_become(_window: Id, _cmd: Sel) -> ObjcBool {
    YES
}

/// Builds — or returns — the `NSWindow` subclass.
///
/// Same shape and same reasoning as [`app`](super::app)'s delegate class,
/// including the already-registered fallback: a null from
/// `objc_allocateClassPair` means the same code is loaded twice in one process,
/// and the class that is already there is ours by name and by method table.
fn window_class() -> Result<Class, ShellError> {
    let outcome = WINDOW.get_or_init(|| {
        let Some(superclass) = ffi::class(c"NSWindow") else {
            return Err(
                "the Objective-C runtime has no NSWindow, so this image has no AppKit \
                        in it"
                    .to_string(),
            );
        };
        // SAFETY: `NSWindow` is a live class, the name is a NUL-terminated
        // literal, and no extra instance storage is asked for.
        let class = unsafe { ffi::objc_allocateClassPair(superclass, WINDOW_CLASS.as_ptr(), 0) };
        if class.is_null() {
            let Some(existing) = ffi::class(WINDOW_CLASS) else {
                return Err(
                    "objc_allocateClassPair refused CrcblWindow and no class of that \
                            name exists"
                        .to_string(),
                );
            };
            log::debug!("the {WINDOW_CLASS:?} class was already registered in this process");
            return Ok(existing as usize);
        }

        let encoding = std::ffi::CString::new(format!("{}@:", ffi::ENC_BOOL))
            .expect("the encoding characters contain no NUL");
        for name in [c"canBecomeKeyWindow", c"canBecomeMainWindow"] {
            // SAFETY: casting a correctly-typed `extern "C"` function to the
            // runtime's opaque `IMP`; the encoding beside it describes the real
            // signature, and the class pair is not yet registered.
            let imp = unsafe {
                core::mem::transmute::<unsafe extern "C" fn(Id, Sel) -> ObjcBool, Imp>(can_become)
            };
            // SAFETY: as above.
            if unsafe { ffi::class_addMethod(class, ffi::sel(name), imp, encoding.as_ptr()) }
                == ffi::NO
            {
                return Err(format!("class_addMethod refused {name:?} on CrcblWindow"));
            }
        }
        // SAFETY: a complete class pair that has not been registered.
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

/// The objects one window is made of, and its geometry at creation.
#[derive(Clone, Copy, Debug)]
pub(super) struct Created {
    /// The `NSWindow`.
    pub window: Id,
    /// Its content view, which owns the layer.
    pub view: Id,
    /// The `CAMetalLayer`, which is the whole of
    /// [`SurfaceTarget::AppKit`](crcbl_core::SurfaceTarget::AppKit).
    pub layer: Id,
    /// `backingScaleFactor`, read back rather than assumed.
    pub scale: f64,
    /// The content area in backing pixels, which AppKit already knows.
    pub size: PhysicalSize,
}

/// A window's content area, in backing pixels.
///
/// `convertRectToBacking:` rather than the arithmetic in
/// [`geometry::physical_size`], deliberately: it is the API that is correct by
/// construction, and the arithmetic exists beside it so that the *relation* is
/// falsifiable on a machine with no Retina display. The macOS suite asserts the
/// two agree, which is a real cross-check at 2× and a tautology at 1× — see
/// [`AppKitShell`]'s tests, which say so.
///
/// # Safety
///
/// `view` must be a live `NSView`, on the main thread.
pub(super) unsafe fn backing_size(view: Id) -> PhysicalSize {
    // SAFETY: both are `NSView` accessors on a live view; `convertRectToBacking:`
    // takes and returns an `NSRect`.
    let backing = unsafe {
        let bounds = ffi::msg_rect(view, ffi::sel(c"bounds"));
        let convert: unsafe extern "C" fn(Id, Sel, NSRect) -> NSRect = ffi::msg_send_stret();
        convert(view, ffi::sel(c"convertRectToBacking:"), bounds)
    };
    geometry::physical_size(backing.size, 1.0)
}

/// A window's `backingScaleFactor`, or 1.0 if it is not on a screen yet.
///
/// # Safety
///
/// `window` must be a live `NSWindow`, on the main thread.
pub(super) unsafe fn backing_scale(window: Id) -> f64 {
    // SAFETY: an `NSWindow` accessor on a live window.
    geometry::usable_scale(unsafe { ffi::msg_f64(window, ffi::sel(c"backingScaleFactor")) })
}

/// Points the layer at the window's current backing store.
///
/// Called at creation and on every resize and scale change. `contentsScale` is
/// what makes Core Animation treat the drawable as high-resolution; without it a
/// Retina window presents a quarter-resolution image scaled up, which looks like
/// a renderer bug and is not one.
///
/// # Safety
///
/// `layer` must be a live `CAMetalLayer` and `view` its live host, on the main
/// thread.
pub(super) unsafe fn size_layer(layer: Id, view: Id, scale: f64) {
    // SAFETY: a live layer; both are `CAMetalLayer`/`CALayer` setters.
    unsafe {
        ffi::msg_send::<unsafe extern "C" fn(Id, Sel, f64)>()(
            layer,
            ffi::sel(c"setContentsScale:"),
            scale,
        );
        let size = backing_size(view);
        ffi::msg_set_size(
            layer,
            ffi::sel(c"setDrawableSize:"),
            NSSize::new(f64::from(size.width), f64::from(size.height)),
        );
    }
}

/// Applies the seam's size constraints to a window.
///
/// All three go straight across: AppKit's limits are on the **content**
/// rectangle and are in points, which is the seam's own request space. See
/// [`geometry::content_limits`].
///
/// # Safety
///
/// `window` must be a live `NSWindow`, on the main thread.
pub(super) unsafe fn apply_constraints(window: Id, constraints: SizeConstraints) {
    let (min, max) = geometry::content_limits(constraints);
    // SAFETY: three `NSWindow` setters, each taking an `NSSize`.
    unsafe {
        ffi::msg_set_size(window, ffi::sel(c"setContentMinSize:"), min);
        ffi::msg_set_size(window, ffi::sel(c"setContentMaxSize:"), max);
        ffi::msg_set_size(
            window,
            ffi::sel(c"setContentAspectRatio:"),
            geometry::aspect_size(constraints.aspect),
        );
    }
}

impl AppKitShell {
    /// Creates the window, its layer-hosting view and its `CAMetalLayer`.
    ///
    /// # Errors
    ///
    /// [`ShellError::Connect`] if a class is missing, or
    /// [`ShellError::WindowCreation`] if AppKit refused to make the window or
    /// the view.
    pub(super) fn create_native_window(
        &self,
        desc: &WindowDesc<'_>,
        target: Option<&Screen>,
    ) -> Result<Created, ShellError> {
        let class = window_class()?;
        let Some(view_class) = ffi::class(c"NSView") else {
            return Err(ShellError::Connect {
                backend: ShellBackend::AppKit,
                detail: "the Objective-C runtime has no NSView".to_string(),
            });
        };
        let Some(layer_class) = ffi::class(c"CAMetalLayer") else {
            return Err(ShellError::Connect {
                backend: ShellBackend::AppKit,
                detail: "the Objective-C runtime has no CAMetalLayer, so this image has no \
                         QuartzCore in it"
                    .to_string(),
            });
        };

        let mask = geometry::style_mask(desc.mode, desc.resizable);
        // A borderless window is the screen's size, not the requested one:
        // `WindowDesc::size` is the *windowed* size and is remembered for the
        // trip back. In points, because that is the space `initWithContentRect:`
        // is in — and, on this platform, the seam's logical space too.
        let content = match (desc.mode, target) {
            (DisplayMode::Borderless { .. }, Some(screen)) => screen.frame,
            (_, Some(screen)) => geometry::centred(screen.visible, geometry::points(desc.size)),
            // No screen to place against — a Mac with no display attached.
            // AppKit puts the window where it likes rather than this backend
            // inventing a desktop layout.
            (_, None) => NSRect::new(0.0, 0.0, desc.size.width, desc.size.height),
        };

        // SAFETY: `alloc` then the designated initialiser. `defer: NO` creates
        // the window device now rather than at first display, which is what
        // makes `backingScaleFactor` and the content view's bounds readable
        // before this function returns.
        let window = unsafe {
            let allocated = ffi::msg(class, ffi::sel(c"alloc"));
            let init: unsafe extern "C" fn(Id, Sel, NSRect, usize, usize, ObjcBool) -> Id =
                ffi::msg_send();
            init(
                allocated,
                ffi::sel(c"initWithContentRect:styleMask:backing:defer:"),
                content,
                mask,
                ffi::value::BACKING_BUFFERED,
                ffi::NO,
            )
        };
        if window.is_null() {
            return Err(ShellError::WindowCreation(
                "-[NSWindow initWithContentRect:styleMask:backing:defer:] returned nil".to_string(),
            ));
        }

        // **Before anything else that could close it.** `releasedWhenClosed`
        // defaults to `YES` on a programmatically created `NSWindow`, which
        // means `close` releases the object — and this backend holds the pointer
        // afterwards, in `destroy_window` and in `Drop`. Leaving it on is a
        // use-after-free that only fires on the close path.
        // SAFETY: a live window; the setter takes a `BOOL`.
        unsafe { ffi::msg_set_bool(window, ffi::sel(c"setReleasedWhenClosed:"), false) };

        // SAFETY: `alloc` then `initWithFrame:` on `NSView`; the frame is in the
        // window's own coordinates, so the origin is zero whatever the window's
        // screen position is.
        let view = unsafe {
            let allocated = ffi::msg(view_class, ffi::sel(c"alloc"));
            let init: unsafe extern "C" fn(Id, Sel, NSRect) -> Id = ffi::msg_send();
            init(
                allocated,
                ffi::sel(c"initWithFrame:"),
                NSRect::new(0.0, 0.0, content.size.width, content.size.height),
            )
        };
        if view.is_null() {
            // SAFETY: releasing the window this function allocated and is about
            // to stop tracking.
            unsafe { ffi::msg_void(window, ffi::sel(c"release")) };
            return Err(ShellError::WindowCreation(
                "-[NSView initWithFrame:] returned nil".to_string(),
            ));
        }

        // SAFETY: `layer` is a class method returning an autoreleased
        // `CAMetalLayer`; it is retained below because this shell hands the
        // pointer out in `SurfaceTarget::AppKit` and must outlive the pool.
        let layer = unsafe { ffi::msg(layer_class, ffi::sel(c"layer")) };
        if layer.is_null() {
            // SAFETY: releasing what this function allocated.
            unsafe {
                ffi::msg_void(view, ffi::sel(c"release"));
                ffi::msg_void(window, ffi::sel(c"release"));
            }
            return Err(ShellError::WindowCreation(
                "+[CAMetalLayer layer] returned nil".to_string(),
            ));
        }
        // SAFETY: a live autoreleased object; the retain is balanced by the
        // release in `AppKitShell::release_window`.
        unsafe { ffi::msg_void(layer, ffi::sel(c"retain")) };

        // **`setLayer:` then `setWantsLayer:`**, in that order, which is what
        // makes the view layer-*hosting*. Backwards, AppKit creates its own
        // `CALayer` first and replaces ours; see the module docs.
        //
        // SAFETY: a live view and a live layer; both setters take what they are
        // given here.
        unsafe {
            ffi::msg1_void(view, ffi::sel(c"setLayer:"), layer);
            ffi::msg_set_bool(view, ffi::sel(c"setWantsLayer:"), true);
            ffi::msg1_void(window, ffi::sel(c"setContentView:"), view);
            // The window retains the content view, so this shell's own
            // reference from `alloc` is handed over.
            ffi::msg_void(view, ffi::sel(c"release"));
        }

        // SAFETY: an autoreleased `NSString`, valid for this pool.
        if let Some(title) = unsafe { ffi::nsstring(desc.title) } {
            // SAFETY: a live window and a live string, which the window copies.
            unsafe { ffi::msg1_void(window, ffi::sel(c"setTitle:"), title) };
        } else {
            return Err(ShellError::InvalidDescriptor(
                "title contains a NUL byte".to_string(),
            ));
        }

        // SAFETY: a live window.
        unsafe { apply_constraints(window, desc.constraints) };

        // SAFETY: a live window; both read.
        let scale = unsafe { backing_scale(window) };
        // SAFETY: a live layer and its live host view.
        unsafe { size_layer(layer, view, scale) };

        if desc.visible {
            // SAFETY: a live window; `nil` is the documented sender.
            unsafe {
                ffi::msg1_void(window, ffi::sel(c"makeKeyAndOrderFront:"), ptr::null_mut());
            }
        }

        Ok(Created {
            window,
            view,
            layer,
            scale,
            // SAFETY: a live view whose window device exists — `defer: NO`.
            size: unsafe { backing_size(view) },
        })
    }

    /// Switches a window between windowed and borderless.
    ///
    /// AppKit is the only party involved — there is no window manager to ask, as
    /// on Win32 and unlike X11 — so this either succeeds or the screen it named
    /// is gone.
    ///
    /// # Errors
    ///
    /// [`ShellError::NoSuchMonitor`] if the named screen was detached since
    /// [`monitors`](crate::Shell::monitors) was called, or
    /// [`ShellError::Backend`] if there is no screen at all to be borderless on.
    pub(super) fn apply_mode(
        &mut self,
        handle: crate::WindowId,
        mode: DisplayMode,
    ) -> Result<(), ShellError> {
        let state = self.window(handle)?;
        let window = state.window;
        let resizable = state.resizable;
        let saved = state.saved;

        match mode {
            DisplayMode::Borderless { monitor } => {
                let frame = self.borderless_frame(window, monitor)?;
                if saved.is_none() {
                    // SAFETY: a live window; both are accessors.
                    let saved = unsafe {
                        super::shell::Saved {
                            mask: ffi::msg_usize(window, ffi::sel(c"styleMask")),
                            frame: ffi::msg_rect(window, ffi::sel(c"frame")),
                        }
                    };
                    self.window_mut(handle)?.saved = Some(saved);
                }
                // SAFETY: a live window. The mask goes first: `setFrame:` places
                // the window for the mask it has, and a borderless window's
                // frame rectangle *is* its content rectangle.
                unsafe {
                    ffi::msg_set_usize(
                        window,
                        ffi::sel(c"setStyleMask:"),
                        geometry::style_mask(mode, resizable),
                    );
                    set_frame(window, frame);
                    // A window that was key stays key, but a borderless one has
                    // to be told to come forward again on some systems — and
                    // asking twice costs nothing.
                    ffi::msg1_void(window, ffi::sel(c"makeKeyAndOrderFront:"), ptr::null_mut());
                }
            }
            DisplayMode::Windowed => {
                let restored = match self.window_mut(handle)?.saved.take() {
                    Some(saved) => saved,
                    None => {
                        // **A window created borderless has nothing saved**, and
                        // there is nothing dishonest to do about it: it has
                        // never had a titled style or a windowed rectangle. So
                        // one is built — the windowed mask, and
                        // `WindowDesc::size` centred on the screen it is
                        // covering. Without this the mask would stay borderless
                        // while the effective mode said windowed, which is the
                        // seam reporting a mode the window is not in.
                        let state = self.window(handle)?;
                        let size = geometry::points(state.requested_size);
                        let area = self
                            .screen_of(window)
                            .and_then(|id| self.screens.iter().find(|screen| screen.id == id))
                            .map_or(NSRect::new(0.0, 0.0, size.width, size.height), |screen| {
                                geometry::centred(screen.visible, size)
                            });
                        super::shell::Saved {
                            mask: geometry::style_mask(DisplayMode::Windowed, resizable),
                            frame: area,
                        }
                    }
                };
                // SAFETY: a live window. Mask first, for the reason the module
                // docs give: restoring the rectangle under a borderless mask
                // would put a titled window where a frameless one fitted.
                unsafe {
                    ffi::msg_set_usize(window, ffi::sel(c"setStyleMask:"), restored.mask);
                    set_frame(window, restored.frame);
                }
            }
        }

        // AppKit has nobody to refuse: the effective mode is what was just
        // applied. The screen is read back rather than assumed, because
        // `Borderless { monitor: None }` means "wherever the window already is"
        // and the answer names it.
        let screen = self.screen_of(window);
        // SAFETY: a live window.
        let scale = unsafe { backing_scale(window) };
        let state = self.window_mut(handle)?;
        state.effective_mode = match mode {
            DisplayMode::Windowed => DisplayMode::Windowed,
            DisplayMode::Borderless { .. } => DisplayMode::Borderless { monitor: screen },
        };
        state.scale = scale;
        // SAFETY: the layer and its host view are alive for as long as the
        // window is, and this is the same thread that created them.
        unsafe { size_layer(state.layer, state.view, scale) };
        self.refresh_presentation();
        Ok(())
    }

    /// The rectangle a borderless window should cover, in AppKit points.
    ///
    /// A named screen is looked up in the enumeration — so a detached one is a
    /// clean [`ShellError::NoSuchMonitor`] rather than a window moved to
    /// nowhere — and `None` means the screen the window is already on, which is
    /// what [`DisplayMode::Borderless`]'s field documents.
    fn borderless_frame(
        &self,
        window: Id,
        monitor: Option<MonitorId>,
    ) -> Result<NSRect, ShellError> {
        if let Some(monitor) = monitor {
            return self
                .screens
                .iter()
                .find(|screen| screen.id == monitor)
                .map(|screen| screen.frame)
                .ok_or(ShellError::NoSuchMonitor(monitor.0));
        }
        self.screen_of(window)
            .and_then(|id| self.screens.iter().find(|screen| screen.id == id))
            // A hotplug between `[window screen]` and the enumeration this shell
            // holds: the primary is a better answer than a failure.
            .or_else(|| self.screens.first())
            .map(|screen| screen.frame)
            .ok_or_else(|| {
                ShellError::Backend(
                    "there is no display to place a borderless window on".to_string(),
                )
            })
    }
}

/// `setFrame:display:` with `display: YES`.
///
/// A one-line helper because the signature is long enough to be worth writing
/// once, and because `display: NO` — which is the tempting choice for something
/// that is about to be redrawn anyway — leaves the old frame on screen until the
/// next event, which reads as a laggy mode switch.
///
/// # Safety
///
/// `window` must be a live `NSWindow`, on the main thread.
unsafe fn set_frame(window: Id, frame: NSRect) {
    // SAFETY: an `NSWindow` setter taking an `NSRect` and a `BOOL`.
    let send: unsafe extern "C" fn(Id, Sel, NSRect, ObjcBool) = unsafe { ffi::msg_send() };
    unsafe { send(window, ffi::sel(c"setFrame:display:"), frame, YES) };
}

/// The `AppWindow` fields a configuration is read out of, in one place so the
/// two callers cannot drift.
///
/// # Safety
///
/// The window and view must be live, on the main thread.
pub(super) unsafe fn configuration_of(state: &AppWindow) -> crate::WindowConfiguration {
    crate::WindowConfiguration {
        // SAFETY: a live view of a live window.
        size: unsafe { backing_size(state.view) },
        scale_factor: state.scale,
        mode: state.effective_mode,
    }
}
