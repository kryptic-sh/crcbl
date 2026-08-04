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
//! # Decision: `constrainFrameRect:toScreen:` is overridden, because a game says where its window goes
//!
//! `-[NSWindow setFrame:display:]` does not simply apply the rectangle it is
//! given: it passes it through `constrainFrameRect:toScreen:` first, whose
//! default implementation keeps a window's title bar clear of the menu bar and
//! the window itself on a screen. That is right for a document window a person
//! dragged and **wrong for every frame this backend sets**, all of which are
//! computed from an `NSScreen` rectangle that is by construction on that screen:
//! a borderless window is meant to cover the menu bar, and a restored windowed
//! one is meant to land exactly where it was.
//!
//! The default is not merely unhelpful, it is silent — it preserves the size it
//! was given and moves the origin, so the symptom is a window of exactly the
//! right extent in the wrong place. **A borderless window that is screen-sized
//! but offset hangs off two edges of the display**, and nothing in the seam can
//! see it: [`WindowState`](crate::WindowState) carries a size and no position, so
//! `set_mode` reported a perfectly correct 1024×768 while the window sat at
//! (192, 160) — its creation origin, untouched.
//!
//! Overriding it to answer the proposed rectangle unchanged is the documented
//! way out, and the subclass to do it in already exists for
//! `canBecomeKeyWindow`. What that gives up is AppKit's safety net for a frame
//! this backend computed wrongly; what it buys is that a frame this backend
//! computed *correctly* is the frame the window gets.
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
use super::view;

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

/// `constrainFrameRect:toScreen:` — **the rectangle it was handed, unchanged**.
///
/// See the [module docs](self) for the argument. In short: every frame this
/// backend sets comes from an `NSScreen` rectangle, and AppKit's default
/// implementation silently moves such a frame to keep a title bar below the menu
/// bar — preserving the size and changing the origin, which is a borderless
/// window of exactly the right extent hanging off two edges of the display.
///
/// `screen` is ignored rather than consulted: the caller has already chosen the
/// screen, and this method exists to stop a second opinion being applied to that
/// choice.
///
/// # Safety
///
/// Called only by AppKit, on an instance of the class built below.
unsafe extern "C" fn constrain_frame(_window: Id, _cmd: Sel, frame: NSRect, _screen: Id) -> NSRect {
    frame
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

        let predicate = std::ffi::CString::new(format!("{}@:", ffi::ENC_BOOL))
            .expect("the encoding characters contain no NUL");
        for name in [c"canBecomeKeyWindow", c"canBecomeMainWindow"] {
            // SAFETY: casting a correctly-typed `extern "C"` function to the
            // runtime's opaque `IMP`; the encoding beside it describes the real
            // signature, and the class pair is not yet registered.
            let imp = unsafe {
                core::mem::transmute::<unsafe extern "C" fn(Id, Sel) -> ObjcBool, Imp>(can_become)
            };
            // SAFETY: as above.
            if unsafe { ffi::class_addMethod(class, ffi::sel(name), imp, predicate.as_ptr()) }
                == ffi::NO
            {
                return Err(format!("class_addMethod refused {name:?} on CrcblWindow"));
            }
        }

        // `NSRect constrainFrameRect:(NSRect)frame toScreen:(NSScreen *)screen`
        // — a rectangle returned, a rectangle and an object taken. See the
        // module docs for why this backend answers it unchanged.
        let constrain = std::ffi::CString::new(format!("{rect}@:{rect}@", rect = ffi::ENC_RECT))
            .expect("the encoding characters contain no NUL");
        // SAFETY: as above. The `NSRect` return is what makes this a separate
        // registration from the two predicates rather than another turn of that
        // loop: it needs its own signature and its own type encoding, and the
        // compiler emits whichever struct-return convention the target uses.
        let imp = unsafe {
            core::mem::transmute::<unsafe extern "C" fn(Id, Sel, NSRect, Id) -> NSRect, Imp>(
                constrain_frame,
            )
        };
        // SAFETY: as above.
        if unsafe {
            ffi::class_addMethod(
                class,
                ffi::sel(c"constrainFrameRect:toScreen:"),
                imp,
                constrain.as_ptr(),
            )
        } == ffi::NO
        {
            return Err(
                "class_addMethod refused constrainFrameRect:toScreen: on CrcblWindow".to_string(),
            );
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
        // **`CrcblView`, not `NSView`.** A plain view answers `NO` to
        // `acceptsFirstResponder`, implements none of the responder methods
        // input arrives through, and conforms to no text-input protocol — so a
        // window built on one is a window that receives nothing. See
        // [`view`](super::view).
        let view_class = view::view_class()?;
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

        // **The switches without which input and drops do not exist**, and none
        // of them is visible to a test that calls a responder method itself —
        // see [`view`](super::view), which is where all five gaps are written
        // out.
        //
        // SAFETY: a live window and its live content view.
        unsafe {
            // Nothing generates `NSEventTypeMouseMoved` for a window that has
            // not asked. Without this, pointer motion with no button held simply
            // does not happen, and `mouseDragged:` covers up the absence for
            // exactly as long as somebody is holding a button.
            ffi::msg_set_bool(window, ffi::sel(c"setAcceptsMouseMovedEvents:"), true);
            // `sendEvent:` delivers key events to the first responder, which is
            // the *window* until something else claims it. `CrcblView` answers
            // `YES` to `acceptsFirstResponder:` precisely so this can succeed.
            let make: unsafe extern "C" fn(Id, Sel, Id) -> ObjcBool = ffi::msg_send();
            if make(window, ffi::sel(c"makeFirstResponder:"), view) == ffi::NO {
                log::warn!(
                    "-[NSWindow makeFirstResponder:] refused the content view; this window \
                     will receive no key events"
                );
            }
            // Enter, exit and cursor updates, none of which any other mechanism
            // produces.
            view::add_tracking_area(view);
            // And the drop gate, which is the same shape: **AppKit sends no
            // dragging message at all to a view that has not registered**, so
            // `accept_drops` being off is enforced by the system rather than by
            // this backend refusing an offer it was given. The shell checks the
            // flag again when it translates a drop, because registration is a
            // property of the view and anything in the process could set it.
            if desc.accept_drops {
                view::register_dragged_types(view);
            }
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

/// `setFrame:display:` with `display: YES`, and a warning if the window did not
/// go there.
///
/// A helper because the signature is long enough to be worth writing once, and
/// because `display: NO` — which is the tempting choice for something that is
/// about to be redrawn anyway — leaves the old frame on screen until the next
/// event, which reads as a laggy mode switch.
///
/// # The readback is here because the seam cannot see a position
///
/// `setFrame:display:` is not obliged to apply what it is given, and when it
/// declines it does so **silently and by moving the origin while keeping the
/// size** — see the [module docs](self) on `constrainFrameRect:toScreen:`, which
/// is the mechanism that shipped a screen-sized borderless window at the wrong
/// place. Nothing downstream can notice: [`WindowState`](crate::WindowState)
/// carries an extent and no origin, so every layer above this one would keep
/// reporting a perfectly correct size.
///
/// So this asks the window where it actually ended up. The override above should
/// make the two always agree; a warning here means it did not take, and it names
/// both rectangles rather than leaving the next reader to infer one of them.
///
/// # Safety
///
/// `window` must be a live `NSWindow`, on the main thread.
unsafe fn set_frame(window: Id, frame: NSRect) {
    // SAFETY: an `NSWindow` setter taking an `NSRect` and a `BOOL`.
    let send: unsafe extern "C" fn(Id, Sel, NSRect, ObjcBool) = unsafe { ffi::msg_send() };
    unsafe { send(window, ffi::sel(c"setFrame:display:"), frame, YES) };

    // SAFETY: a live window; `frame` is an accessor returning an `NSRect`.
    let applied = unsafe { ffi::msg_rect(window, ffi::sel(c"frame")) };
    if applied != frame {
        log::warn!(
            "-[NSWindow setFrame:display:] was asked for {frame:?} and the window is at \
             {applied:?}; a matching size with a different origin is AppKit constraining the \
             rectangle, which CrcblWindow overrides constrainFrameRect:toScreen: to prevent"
        );
    }
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

/// That `CrcblWindow` really carries the three overrides it is built for.
///
/// The same shape and the same argument as [`view`](super::view)'s class suite:
/// `objc_allocateClassPair` and `class_addMethod` are Objective-C **runtime**
/// calls, thread-safe and needing no `NSApplication`, so a spawned `#[test]`
/// body may build this class and ask what it implements — see
/// [`session_support`](crate::appkit::session_support) for the rule and for what
/// it costs to get it wrong.
///
/// What it catches is a method the runtime **refused** and a selector spelled
/// wrong, both of which leave the superclass's implementation in place and fail
/// silently: a `canBecomeKeyWindow` that never runs is a borderless window that
/// stops taking keystrokes, and a `constrainFrameRect:toScreen:` that never runs
/// is a borderless window at the wrong origin — which is a defect that shipped
/// and that only `tests/appkit_session.rs`'s AppKit readback found.
#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn the_window_class_installs_every_override_a_borderless_window_needs() {
        let class = window_class().expect("CrcblWindow is built from runtime calls alone");

        for selector in [
            // Without these two a borderless window cannot take the keyboard or
            // the menu bar, because `NSWindowStyleMaskBorderless` is zero.
            c"canBecomeKeyWindow",
            c"canBecomeMainWindow",
            // And without this one AppKit is entitled to move any frame this
            // backend sets, keeping the size and changing the origin.
            c"constrainFrameRect:toScreen:",
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
            assert!(installed, "CrcblWindow does not implement {selector:?}");
        }
    }
}
