//! [`ShellCaps`] — what a backend can actually do, asked as a question about
//! capability rather than about identity.
//!
//! # The regression this exists to prevent
//!
//! ```ignore
//! if cfg!(target_os = "linux") && std::env::var("WAYLAND_DISPLAY").is_ok() {
//!     // …use the viewport path
//! }
//! ```
//!
//! That code is wrong three ways and all three are invisible until someone
//! files a bug: it is wrong on a Wayland compositor that does not implement
//! `wp_viewporter`, wrong under XWayland, and wrong the day the shell grows a
//! backend nobody thought about. `docs/plan/15-windowing.md` is explicit — "the
//! renderer picks blit-vs-viewport and windowed-aspect behavior from caps, never
//! from *am I on Wayland*" — and this module is the mechanism. It mirrors
//! `crcbl-hal`'s `Features`, where the renderer asks for `DRAW_INDIRECT_COUNT`
//! rather than for Vulkan.
//!
//! # Caps are per-shell, not per-window
//!
//! A capability here is a property of the connection and the protocols the
//! server advertised, so it is fixed for the shell's lifetime and reported by
//! [`Shell::caps`](crate::Shell::caps) rather than per window. The one thing
//! that genuinely varies per window — its current size and scale — is
//! [`WindowState`](crate::WindowState), not a capability.
//!
//! There is a real edge here, and it is stated rather than hidden: a Wayland
//! compositor can bind a global *after* startup, so a protocol could in
//! principle appear mid-session. Backends must not flip caps bits when that
//! happens; a renderer that picked the blit path at init cannot be told
//! halfway through a frame that it should have picked the viewport path.
//! Capabilities are latched at connection time, and a missing protocol stays
//! missing until the next run.

bitflags::bitflags! {
    /// What a shell backend supports.
    ///
    /// Each flag names a *behaviour a consumer would otherwise have to guess
    /// from the platform*. A flag that no consumer would branch on does not
    /// belong here.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
    pub struct ShellCaps: u32 {
        /// The compositor can scale a smaller buffer to the window's size in
        /// display hardware, so borderless render-scale costs no shader work.
        ///
        /// Wayland's `wp_viewporter`; the browser's CSS-sized canvas. Absent on
        /// X11, where the renderer must do an upscale blit instead. This is the
        /// exact decision `docs/plan/15-windowing.md` calls out as the reason
        /// caps exist.
        const HW_UPSCALE = 1 << 0;

        /// The window system will enforce an aspect ratio during an interactive
        /// resize.
        ///
        /// X11 `WM_NORMAL_HINTS`, Win32 `WM_SIZING`, macOS
        /// `setContentAspectRatio`. **Wayland has no aspect hint**, so a
        /// Wayland backend clears this bit and the renderer letterboxes.
        /// Letterboxing must work regardless — a tiling window manager ignores
        /// the hint on every backend that has one — so this flag chooses
        /// between "good" and "correct", never between "works" and "broken".
        const ASPECT_HINT_HONORED = 1 << 1;

        /// [`Shell::warp_pointer`](crate::Shell::warp_pointer) moves the
        /// pointer.
        ///
        /// X11 `XWarpPointer`, Win32 `SetCursorPos`. Wayland deliberately
        /// forbids it (a client that can move the cursor can spoof clicks); the
        /// engine's answer there is pointer lock plus relative motion, which is
        /// what a camera wanted anyway.
        const POINTER_WARP = 1 << 2;

        /// [`PointerMode::Locked`](crate::PointerMode) is available: the
        /// pointer can be hidden and pinned while relative motion keeps
        /// flowing.
        ///
        /// Wayland `pointer-constraints` + `relative-pointer`, X11 pointer
        /// grab, Win32 clip-and-recentre, the browser's Pointer Lock API.
        ///
        /// The browser is the one that cannot take the lock when asked: it
        /// grants `requestPointerLock` only inside a user gesture, so
        /// [`set_pointer_mode`](crate::Shell::set_pointer_mode) there records
        /// the request and the player's next click completes it. The bit still
        /// means what it says — the mode is available — and a consumer that
        /// needs to know whether the pointer is pinned *now* reads the shape of
        /// the motion events, as
        /// [`PointerMode::Locked`](crate::PointerMode::Locked) describes.
        const POINTER_LOCK = 1 << 3;

        /// [`PointerMode::Confined`](crate::PointerMode) is available: the
        /// pointer stays inside the window but keeps moving visibly.
        ///
        /// A separate bit from [`POINTER_LOCK`](Self::POINTER_LOCK) because
        /// Wayland's `pointer-constraints` advertises lock and confine
        /// independently, and a strategy game wants confine specifically.
        const POINTER_CONFINE = 1 << 4;

        /// Relative pointer motion is delivered separately from absolute
        /// position, unaccelerated and unclamped by the screen edge.
        ///
        /// This is what a first-person camera must read;
        /// `docs/plan/15-windowing.md` makes raw motion a P0 feature, not an
        /// afterthought, on the grounds that an esports-grade engine cannot
        /// derive aim from clamped absolute positions. Without this bit,
        /// [`ShellEvent::PointerMotion`](crate::ShellEvent::PointerMotion)
        /// carries `raw_delta: None` and the camera has to difference absolute
        /// positions — which is wrong at the screen edge and wrong under
        /// pointer acceleration.
        ///
        /// # The one platform that can decline the bypass
        ///
        /// On the desktop backends the unaccelerated stream is a separate
        /// device interface and either exists or does not. In a browser it is
        /// an *option* on the lock — `requestPointerLock({ unadjustedMovement:
        /// true })` — and a browser that does not implement it hands back the
        /// OS-adjusted deltas instead, either by ignoring the key or by
        /// rejecting with `NotSupportedError` and being asked again without it.
        /// The web backend sets this bit because it reads a relative stream and
        /// asks for it unadjusted, which is the decision a camera branches on:
        /// differencing absolute positions is wrong there whatever the browser
        /// answered. It is not a promise that every browser granted the bypass;
        /// the `web` backend's module docs name which browsers do.
        const RAW_POINTER_MOTION = 1 << 5;

        /// Composed text arrives as
        /// [`ShellEvent::TextCommit`](crate::ShellEvent::TextCommit) through a
        /// real input method, so CJK and dead-key input work.
        ///
        /// Without it a backend still emits `TextCommit` for plain typing —
        /// the event is not IME-only — but complex scripts will not compose.
        /// Full IME (pre-edit strings, candidate windows) is post-MVP; this bit
        /// says only that the commit path is wired to an input method.
        const TEXT_IME = 1 << 6;

        /// Clipboard get/set works. See
        /// [`clipboard`](crate::clipboard).
        ///
        /// Explicitly a capability because the browser gates clipboard reads
        /// behind a permission prompt and a user gesture, which
        /// `docs/plan/15-windowing.md` calls out by name: the editor UI has to
        /// degrade gracefully in-browser rather than offering a paste button
        /// that cannot work.
        const CLIPBOARD = 1 << 7;

        /// Files dragged onto a window arrive as
        /// [`ShellEvent::DroppedFile`](crate::ShellEvent::DroppedFile).
        const DRAG_DROP = 1 << 8;

        /// The shell can create more than one window.
        ///
        /// The MVP editor is single-window, and a canvas backend has exactly
        /// one canvas, so this is not universal. A consumer that wants a
        /// tear-off panel checks this rather than assuming.
        const MULTI_WINDOW = 1 << 9;

        /// Window position in desktop coordinates can be read and set.
        ///
        /// **Wayland cannot do either** — a client is not told where it is and
        /// cannot ask to be moved. Anything that wants to restore a window's
        /// position across sessions (the editor, topic 14's settings store)
        /// must check this bit and degrade to "the compositor decides".
        const WINDOW_POSITION = 1 << 10;

        /// The window system draws the title bar and borders.
        ///
        /// Wayland with `xdg-decoration` and a compositor that opts in, and
        /// every other desktop platform. Without it the UI layer has to draw
        /// its own decorations, which is a layout decision, not a rendering
        /// detail — so it belongs in caps rather than being discovered by
        /// noticing the window looks wrong.
        const SERVER_DECORATIONS = 1 << 11;

        /// Scale factors other than whole integers are reported.
        ///
        /// Wayland `fractional-scale-v1` (in 1/120ths), Win32 per-monitor-v2,
        /// browser `devicePixelRatio`. Without it the shell reports integer
        /// scales only, and a 125% desktop shows up as 1.0 or 2.0 — which the
        /// UI needs to know before it decides whether to snap its layout to a
        /// pixel grid.
        const FRACTIONAL_SCALE = 1 << 12;

        /// [`Shell::wait_events`](crate::Shell::wait_events) genuinely blocks.
        ///
        /// An editor idling at zero frames per second on battery depends on
        /// it; a game never calls it. Backends that cannot block — the browser
        /// above all, where blocking the main thread is not a thing that
        /// happens — leave this clear and their `wait_events` returns
        /// immediately, which is always a correct implementation.
        const EVENT_WAIT = 1 << 13;

        /// Fingers arrive as [`ShellEvent::Touch`](crate::ShellEvent::Touch),
        /// with a contact id and a phase, so more than one can be tracked at a
        /// time.
        ///
        /// **Clear on every desktop backend here**, and that is a statement
        /// about the backends rather than about the platforms: a Windows laptop
        /// and a Linux tablet both have touchscreens, and X11 (XInput2
        /// `XI_TouchBegin`), Wayland (`wl_touch`), Win32 (`WM_POINTERDOWN`) and
        /// AppKit (`NSTouch`) all have a way to deliver them. None of those
        /// paths is written, so none of those backends sets this bit and a game
        /// on them sees no contacts at all — an input that is *absent*, which is
        /// a different thing from one that silently fails.
        ///
        /// A backend that does set it owes the emulated pointer stream for the
        /// primary contact as well; [`ShellEvent::Touch`](crate::ShellEvent::Touch)
        /// says why, and that obligation is what stops a backend turning this on
        /// from breaking every game that binds only the pointer.
        const TOUCH = 1 << 14;
    }
}

impl ShellCaps {
    /// The set a full-featured desktop backend is expected to report.
    ///
    /// Not a promise about any particular platform — it is the reference point
    /// tests and [`HeadlessShell`](crate::HeadlessShell) start from, and the
    /// value a doc example can name without pretending to know what the machine
    /// running it supports.
    pub const DESKTOP: Self = Self::all();

    /// Whether pointer capture in either form is available.
    #[inline]
    #[must_use]
    pub const fn has_pointer_capture(self) -> bool {
        self.intersects(Self::POINTER_LOCK.union(Self::POINTER_CONFINE))
    }

    /// Whether a first-person camera can get trustworthy aim input: the pointer
    /// can be locked *and* relative motion is delivered.
    ///
    /// Both halves are required, and this helper exists because forgetting the
    /// second one produces a camera that works until the pointer reaches the
    /// edge of the screen.
    #[inline]
    #[must_use]
    pub const fn has_mouselook(self) -> bool {
        self.contains(Self::POINTER_LOCK.union(Self::RAW_POINTER_MOTION))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mouselook_needs_both_halves() {
        assert!(ShellCaps::DESKTOP.has_mouselook());
        // Pointer lock alone is the trap: aim breaks at the screen edge.
        assert!(!ShellCaps::POINTER_LOCK.has_mouselook());
        assert!(!ShellCaps::RAW_POINTER_MOTION.has_mouselook());
        assert!((ShellCaps::POINTER_LOCK | ShellCaps::RAW_POINTER_MOTION).has_mouselook());
    }

    #[test]
    fn pointer_capture_is_either_form() {
        assert!(ShellCaps::POINTER_LOCK.has_pointer_capture());
        assert!(ShellCaps::POINTER_CONFINE.has_pointer_capture());
        assert!(!ShellCaps::POINTER_WARP.has_pointer_capture());
        assert!(!ShellCaps::empty().has_pointer_capture());
    }

    #[test]
    fn a_wayland_shaped_cap_set_is_expressible() {
        // The point of the type: the exact combination Wayland produces — hardware
        // upscale yes, aspect hint no, pointer warp no — is a value, not a
        // `#[cfg]`.
        let wayland = ShellCaps::HW_UPSCALE
            | ShellCaps::POINTER_LOCK
            | ShellCaps::POINTER_CONFINE
            | ShellCaps::RAW_POINTER_MOTION
            | ShellCaps::TEXT_IME
            | ShellCaps::CLIPBOARD
            | ShellCaps::DRAG_DROP
            | ShellCaps::MULTI_WINDOW
            | ShellCaps::SERVER_DECORATIONS
            | ShellCaps::FRACTIONAL_SCALE
            | ShellCaps::EVENT_WAIT;
        assert!(wayland.has_mouselook());
        assert!(!wayland.contains(ShellCaps::POINTER_WARP));
        assert!(!wayland.contains(ShellCaps::ASPECT_HINT_HONORED));
        assert!(!wayland.contains(ShellCaps::WINDOW_POSITION));
        // `wl_touch` exists and this backend does not bind it, which is the
        // whole difference between a platform and a backend.
        assert!(!wayland.contains(ShellCaps::TOUCH));
        assert!(ShellCaps::DESKTOP.contains(wayland));
        assert_eq!(ShellCaps::default(), ShellCaps::empty());
    }
}
