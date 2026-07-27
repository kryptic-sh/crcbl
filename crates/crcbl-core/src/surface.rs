//! [`SurfaceTarget`] — the one sanctioned platform leak in the engine.
//!
//! # Why this lives in `crcbl-core`
//!
//! `docs/plan/15-windowing.md` describes `SurfaceTarget` as "the *single*
//! sanctioned platform leak": the shell produces it, a HAL backend consumes it,
//! and nothing in between is allowed to look inside. That makes it **shared
//! vocabulary between two crates that must not depend on each other**:
//!
//! ```text
//!        crcbl-core  (SurfaceTarget lives here)
//!           ^     ^
//!           |     |
//!   crcbl-shell   crcbl-hal
//!                    ^
//!                    |
//!            crcbl-vk / crcbl-wgpu / …
//! ```
//!
//! * `crcbl-shell` must not depend on `crcbl-hal` — a windowing library that
//!   needs a graphics HAL to compile has the dependency backwards, and
//!   `HeadlessShell` would drag a backend seam into CI-only builds.
//! * `crcbl-hal` must not depend on `crcbl-shell` — backends are used offscreen
//!   (`crcbl screenshot`, golden-image e2e) with no window anywhere in the
//!   process, and `crcbl-shell` will grow real platform code that a headless
//!   render test has no business linking.
//!
//! Both already depend on `crcbl-core`, so putting the type here is the only
//! placement that does not create a cycle or a spurious dependency. It costs
//! `crcbl-core` nothing: the type is a plain enum of integers and raw pointers
//! with no methods that touch them.
//!
//! # Safety model
//!
//! Constructing a `SurfaceTarget` is **safe**. Nothing here dereferences a
//! pointer, and a wrong value is inert until a backend uses it. The obligation
//! sits at the single point that can act on it — `crcbl-hal`'s
//! `Instance::create_surface`, which is `unsafe` and documents the full
//! contract (handles are live objects of the stated kind, and they outlive the
//! surface created from them).
//!
//! This mirrors `raw-window-handle` 0.6 and `wgpu::Instance::create_surface_unsafe`,
//! and it means this module — the one place platform pointers enter the engine
//! — contains no `unsafe` at all.
//!
//! # Thread safety
//!
//! `SurfaceTarget` is deliberately **not** `Send`/`Sync` (it holds
//! [`NonNull`]). Several of these handles are thread-affine in ways we cannot
//! promise away — a Win32 `HWND` belongs to the thread that pumps its message
//! queue, and an `NSView`/`CAMetalLayer` is main-thread-only on macOS. The
//! practical consequence is the one every engine already lives with: create the
//! HAL surface on the thread that owns the window. `raw-window-handle` reached
//! the same conclusion in 0.6 after shipping the unsound `Send` version.

use core::ffi::c_void;
use core::ptr::NonNull;

/// A window's native presentation handles, in the form a HAL backend needs to
/// create a surface.
///
/// Produced by `crcbl-shell`, destructured *only* by HAL backends. Engine code,
/// samples and the editor pass it through opaquely; a `#[cfg]` on a platform in
/// a consumer crate is a regression.
///
/// # Exhaustiveness
///
/// This enum is intentionally **not** `#[non_exhaustive]`. Adding a platform
/// should break every backend's `match` loudly at compile time — the whole
/// engine is one workspace, and a silently-ignored new platform is exactly the
/// failure this type exists to prevent.
///
/// # Variants and their producers
///
/// | Variant | Shell backend | Vulkan entry point |
/// | --- | --- | --- |
/// | [`Wayland`](Self::Wayland) | Wayland (P0) | `vkCreateWaylandSurfaceKHR` |
/// | [`Xcb`](Self::Xcb) | X11 (P0) | `vkCreateXcbSurfaceKHR` |
/// | [`Web`](Self::Web) | canvas (P5) | — (WebGPU `GPUCanvasContext`) |
/// | [`Win32`](Self::Win32) | Win32 (P14) | `vkCreateWin32SurfaceKHR` |
/// | [`AppKit`](Self::AppKit) | AppKit (P14) | `vkCreateMetalSurfaceEXT` |
/// | [`Offscreen`](Self::Offscreen) | `HeadlessShell` | — (image ring, no WSI) |
///
/// Xlib is absent on purpose: topic 15 links `libxcb`, not `libX11`, so no
/// shell backend can produce an `Xlib` target and a variant nobody constructs
/// is a variant every backend still has to handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SurfaceTarget {
    /// Wayland: `wl_display*` + `wl_surface*`, as returned by
    /// `libwayland-client` (topic 15's WSI-ABI exception — the driver calls
    /// libwayland functions on these).
    Wayland {
        /// `struct wl_display *`.
        display: NonNull<c_void>,
        /// `struct wl_surface *`.
        surface: NonNull<c_void>,
    },
    /// X11 through XCB: `xcb_connection_t*` plus the window's XID.
    Xcb {
        /// `xcb_connection_t *`.
        connection: NonNull<c_void>,
        /// `xcb_window_t` — an XID, not a pointer.
        window: u32,
        /// `xcb_visualid_t` of the window, needed by some backends to pick a
        /// compatible format.
        visual_id: u32,
    },
    /// Windows: module instance + window handle.
    Win32 {
        /// `HINSTANCE` of the module that registered the window class.
        hinstance: NonNull<c_void>,
        /// `HWND`.
        hwnd: NonNull<c_void>,
    },
    /// macOS/iOS: a `CAMetalLayer*`.
    ///
    /// The shell creates and owns the layer (attaching it to its `NSView`), so
    /// backends never have to touch AppKit — Metal takes the layer directly and
    /// `VK_EXT_metal_surface` takes it through MoltenVK.
    AppKit {
        /// `CAMetalLayer *`.
        layer: NonNull<c_void>,
    },
    /// Web: an integer key into the shell's JS-side canvas registry.
    ///
    /// Topic 15 specifies a hand-rolled JS shim rather than `wasm-bindgen`, so
    /// the canvas is identified by a number the shim assigns — no string
    /// allocation crosses the wasm boundary, and this variant stays `Copy`.
    Web {
        /// Registry key handed out by the shell's JS shim.
        canvas_id: u32,
    },
    /// No window at all: render into a backend-managed image ring and read
    /// back.
    ///
    /// `HeadlessShell` (topic 15) and `crcbl screenshot` (topic 11) need a
    /// target to hand the HAL even though there is nothing to present to.
    /// Backends implement this as a swapchain-shaped rotation of offscreen
    /// images, which is what makes the P1 golden-image e2e run through the
    /// *same* acquire/present code path as a real window instead of a
    /// second, less-tested one.
    Offscreen {
        /// Width in physical pixels.
        width: u32,
        /// Height in physical pixels.
        height: u32,
    },
}

impl SurfaceTarget {
    /// A short, stable name for the platform this target names.
    ///
    /// For logs and error messages — `"wayland"`, `"xcb"`, `"win32"`,
    /// `"appkit"`, `"web"`, `"offscreen"`.
    #[must_use]
    pub const fn platform_name(&self) -> &'static str {
        match self {
            Self::Wayland { .. } => "wayland",
            Self::Xcb { .. } => "xcb",
            Self::Win32 { .. } => "win32",
            Self::AppKit { .. } => "appkit",
            Self::Web { .. } => "web",
            Self::Offscreen { .. } => "offscreen",
        }
    }

    /// Whether this target presents to a real window server.
    ///
    /// `false` only for [`SurfaceTarget::Offscreen`]. Backends use it to skip
    /// WSI extension checks and present-queue selection.
    #[must_use]
    pub const fn is_windowed(&self) -> bool {
        !matches!(self, Self::Offscreen { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dangling() -> NonNull<c_void> {
        NonNull::dangling()
    }

    #[test]
    fn platform_names_are_distinct_and_stable() {
        let targets = [
            SurfaceTarget::Wayland {
                display: dangling(),
                surface: dangling(),
            },
            SurfaceTarget::Xcb {
                connection: dangling(),
                window: 1,
                visual_id: 2,
            },
            SurfaceTarget::Win32 {
                hinstance: dangling(),
                hwnd: dangling(),
            },
            SurfaceTarget::AppKit { layer: dangling() },
            SurfaceTarget::Web { canvas_id: 7 },
            SurfaceTarget::Offscreen {
                width: 320,
                height: 200,
            },
        ];
        let mut names: Vec<_> = targets.iter().map(SurfaceTarget::platform_name).collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "platform names must be unique");
    }

    #[test]
    fn only_offscreen_is_unwindowed() {
        assert!(
            SurfaceTarget::Web { canvas_id: 0 }.is_windowed(),
            "a canvas is a real presentation target"
        );
        assert!(
            !SurfaceTarget::Offscreen {
                width: 1,
                height: 1,
            }
            .is_windowed()
        );
    }

    #[test]
    fn target_is_copy_and_comparable() {
        let a = SurfaceTarget::Web { canvas_id: 3 };
        let b = a;
        assert_eq!(a, b);
        assert_ne!(a, SurfaceTarget::Web { canvas_id: 4 });
    }

    // The deliberate non-`Send`/non-`Sync` property from the module docs is
    // enforced structurally rather than by a test: [`NonNull`] is `!Send` and
    // `!Sync`, so the only way to widen it is a hand-written `unsafe impl` in
    // this file. Asserting the *absence* of an auto trait needs either
    // specialization or a `trybuild` compile-fail fixture, and neither is worth
    // a dependency to restate what the type system already guarantees. If you
    // are here to add that `unsafe impl`, read the module docs first.
}
