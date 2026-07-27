//! Windows: how one is asked for, what it can be told to do, and what it
//! currently is.
//!
//! # Two display modes, and no third
//!
//! [`DisplayMode`] has exactly two variants because
//! `docs/plan/15-windowing.md` locks it at two. Exclusive/true fullscreen — the
//! mode that changes the display's video timing — is **dropped by decision**,
//! not deferred:
//!
//! * Wayland cannot modeset from a client, by design.
//! * macOS fights it.
//! * On Windows, borderless plus the DXGI flip model gets independent-flip and
//!   MPO latency close enough to exclusive that the remaining difference does
//!   not buy back the alt-tab disasters, the mode-restore-on-crash problem, or
//!   the multi-monitor breakage.
//!
//! Resolution control lives one layer up instead: in
//! [`Borderless`](DisplayMode::Borderless) the renderer draws to an offscreen
//! target at whatever internal resolution it wants and upscales to the native
//! surface, either with a blit or — where
//! [`ShellCaps::HW_UPSCALE`](crate::ShellCaps::HW_UPSCALE) is set — by handing
//! the compositor the smaller buffer. **The shell only ever presents
//! native-size surfaces.** A `DisplayMode` that carried a resolution would be a
//! promise this crate cannot keep.
//!
//! If a "true fullscreen" request ever arrives, the answer is that it is this
//! enum's absence of a variant, not a `ShellError::Unsupported`.
//!
//! # Aspect ratio is a constraint, not a mode
//!
//! `15-windowing.md`'s table describes windowed mode as "freeform **or
//! aspect-locked**", which reads like two modes. It is one mode with a
//! constraint, and the constraint lives in [`SizeConstraints`] alongside
//! min/max — where the trait sketch's `set_constraints(win, c) // min/max/aspect`
//! already put it. Modelling it in both places would create a question with no
//! good answer: if `DisplayMode::Windowed { aspect: Some(16:9) }` and
//! `SizeConstraints { aspect: Some(4:3) }` disagree, which wins? Keeping one
//! home also means an aspect lock survives a round trip through borderless and
//! back, which is what a player toggling modes expects.

use core::fmt;

use crate::{AspectRatio, LogicalSize, MonitorId, PhysicalSize};

/// Marker type for the [`Handle`](crcbl_core::Handle) that names a window.
///
/// Uninhabited: it exists only to give [`WindowId`] a distinct type, exactly as
/// `crcbl-hal`'s resource markers do. There is no `Window` struct to hold — the
/// real object lives inside the backend.
#[derive(Debug)]
pub enum Window {}

/// Identifier of a window created by a [`Shell`](crate::Shell).
///
/// A generational [`Handle`](crcbl_core::Handle), so using one after
/// [`destroy_window`](crate::Shell::destroy_window) is a clean
/// [`ShellError::InvalidWindow`](crate::ShellError::InvalidWindow) rather than
/// an operation applied to whatever window took the slot.
pub type WindowId = crcbl_core::Handle<Window>;

/// How a window is presented.
///
/// Two variants, deliberately. See the [module docs](self).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DisplayMode {
    /// A decorated window the user can move and resize.
    ///
    /// Render resolution is the client area, 1:1; a resize recreates the
    /// swapchain. Whether the decorations are drawn by the window system or by
    /// the engine's UI depends on
    /// [`ShellCaps::SERVER_DECORATIONS`](crate::ShellCaps::SERVER_DECORATIONS).
    #[default]
    Windowed,

    /// A frameless window covering a monitor, with the desktop's video mode
    /// untouched.
    Borderless {
        /// Which monitor to cover, or `None` for "wherever the window already
        /// is".
        ///
        /// `None` is not a lazy default — it is the only thing a Wayland
        /// backend can honour, since a client cannot position its own surface
        /// and can only ask `xdg_toplevel.set_fullscreen` with an output hint
        /// the compositor may ignore. A caller that genuinely needs a specific
        /// monitor checks
        /// [`ShellCaps::WINDOW_POSITION`](crate::ShellCaps::WINDOW_POSITION)
        /// first.
        monitor: Option<MonitorId>,
    },
}

impl DisplayMode {
    /// Whether this is [`DisplayMode::Borderless`].
    #[inline]
    #[must_use]
    pub const fn is_borderless(self) -> bool {
        matches!(self, Self::Borderless { .. })
    }
}

impl fmt::Display for DisplayMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Windowed => f.write_str("windowed"),
            Self::Borderless { monitor: None } => f.write_str("borderless"),
            Self::Borderless {
                monitor: Some(monitor),
            } => write!(f, "borderless on {monitor}"),
        }
    }
}

/// Limits the window system should enforce on an interactive resize.
///
/// All three are hints in the strict sense: a tiling window manager honours
/// none of them, and Wayland has no aspect hint at all. Consumers must stay
/// correct when they are ignored — which for aspect means letterboxing
/// in-renderer with [`AspectRatio::fit`], the universal fallback.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SizeConstraints {
    /// Smallest client area the user may resize to.
    pub min: Option<LogicalSize>,
    /// Largest client area the user may resize to.
    pub max: Option<LogicalSize>,
    /// Ratio the client area should keep during a resize.
    ///
    /// Honoured natively only where
    /// [`ShellCaps::ASPECT_HINT_HONORED`](crate::ShellCaps::ASPECT_HINT_HONORED)
    /// is set. Where it is not, a backend picks the nearest aspect-correct size
    /// it is allowed to pick and the renderer letterboxes the remainder.
    pub aspect: Option<AspectRatio>,
}

impl SizeConstraints {
    /// No constraints at all.
    pub const NONE: Self = Self {
        min: None,
        max: None,
        aspect: None,
    };

    /// Constraints with a minimum size and nothing else.
    #[must_use]
    pub const fn min(size: LogicalSize) -> Self {
        Self {
            min: Some(size),
            max: None,
            aspect: None,
        }
    }

    /// These constraints with an aspect lock added.
    #[must_use]
    pub const fn with_aspect(mut self, aspect: AspectRatio) -> Self {
        self.aspect = Some(aspect);
        self
    }

    /// Applies the constraints to a physical size, as a backend that cannot
    /// delegate to the window system would.
    ///
    /// Clamping happens before the aspect fit, so the result never exceeds
    /// `max`. It can fall below `min` when an aspect lock demands it: a size
    /// that violates the aspect ratio is visibly wrong, while one a few pixels
    /// under the minimum is not, and something has to give when the two
    /// disagree.
    ///
    /// [`HeadlessShell`](crate::HeadlessShell) applies this, which is what
    /// makes constraint behaviour testable without a window manager.
    #[must_use]
    pub fn apply(&self, size: PhysicalSize, scale_factor: f64) -> PhysicalSize {
        let mut result = size;
        if let Some(min) = self.min {
            let min = min.to_physical(scale_factor);
            result.width = result.width.max(min.width);
            result.height = result.height.max(min.height);
        }
        if let Some(max) = self.max {
            let max = max.to_physical(scale_factor);
            result.width = result.width.min(max.width);
            result.height = result.height.min(max.height);
        }
        if let Some(aspect) = self.aspect {
            result = aspect.fit(result);
        }
        result
    }
}

/// Everything needed to create a window.
///
/// POD apart from the borrowed title, and `Copy`-free only because of it. The
/// lifetime is deliberate: a shell never needs to own the string past the call,
/// and forcing a `String` on every caller would allocate on a path that runs
/// once.
#[derive(Clone, Copy, Debug)]
pub struct WindowDesc<'a> {
    /// Title bar text. Also what the taskbar and window switcher show.
    pub title: &'a str,

    /// Application identifier, e.g. `"sh.kryptic.crcbl.sandbox"`.
    ///
    /// Wayland's `xdg_toplevel.set_app_id` and X11's `WM_CLASS`. This is how a
    /// desktop environment matches a window to its `.desktop` file for the icon
    /// and the taskbar grouping, and getting it wrong is why some applications
    /// show a generic gear icon. There is no useful default, so it is
    /// required.
    pub app_id: &'a str,

    /// Requested client-area size, in logical units.
    ///
    /// A request, not a demand — a tiling window manager will size the window
    /// however it likes and the first
    /// [`ShellEvent::Resized`](crate::ShellEvent::Resized) is the authority.
    /// Never size a swapchain from this field.
    pub size: LogicalSize,

    /// Resize limits and aspect lock; see [`SizeConstraints`].
    pub constraints: SizeConstraints,

    /// Windowed or borderless at creation.
    ///
    /// Setting it here rather than creating windowed and immediately switching
    /// avoids a visible flash of a decorated window, which is what every engine
    /// that gets this wrong looks like on startup.
    pub mode: DisplayMode,

    /// Whether the window is resizable at all.
    ///
    /// Distinct from `min == max` in [`SizeConstraints`]: this also removes the
    /// maximize button and the resize handles, which is a different visual
    /// result even where the enforced size is identical.
    pub resizable: bool,

    /// Whether to map the window immediately.
    ///
    /// `false` creates it hidden, so the first frame can be rendered before
    /// anything is shown — the fix for the black-window flash on startup.
    /// Reveal it with
    /// [`set_visible`](crate::Shell::set_visible) once there is something to
    /// look at.
    pub visible: bool,

    /// Whether files dropped on the window should produce
    /// [`ShellEvent::DroppedFile`](crate::ShellEvent::DroppedFile).
    ///
    /// Off by default because accepting a drop means advertising it to the
    /// window system, and a game that ignores drops should not show a
    /// drop cursor.
    pub accept_drops: bool,
}

impl Default for WindowDesc<'_> {
    fn default() -> Self {
        Self {
            title: "Crucible",
            app_id: "sh.kryptic.crcbl",
            size: LogicalSize::new(1280.0, 720.0),
            constraints: SizeConstraints::NONE,
            mode: DisplayMode::Windowed,
            resizable: true,
            visible: true,
            accept_drops: false,
        }
    }
}

/// What the window system has actually told us about a window.
///
/// The three facts arrive **together or not at all**, which is why they are one
/// struct rather than three fields on [`WindowState`]. Every backend delivers
/// them as a single message — Wayland's `xdg_surface.configure`, Win32's
/// `WM_DPICHANGED` with its suggested rectangle, AppKit's
/// `windowDidChangeBackingProperties` — and a consumer that could hold a size
/// from one configure and a scale from another would compute the wrong logical
/// size for exactly one frame, which is the kind of bug that gets closed as
/// "cannot reproduce".
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowConfiguration {
    /// Client area in device pixels — what the swapchain must match.
    pub size: PhysicalSize,
    /// Device pixels per logical unit for this window.
    pub scale_factor: f64,
    /// The mode actually in effect.
    ///
    /// Not necessarily [`WindowState::requested_mode`]: a compositor may refuse
    /// a fullscreen request, and a tiling window manager makes the whole
    /// question moot. This is the answer, that is the question.
    pub mode: DisplayMode,
}

/// A window's current, observable state.
///
/// Returned by [`Shell::window_state`](crate::Shell::window_state) as a snapshot
/// by value. It exists so a consumer that missed an event — a system that
/// started late, an editor panel that was just opened — can ask, instead of
/// having to have watched the whole event stream from the beginning.
///
/// It is **not** a substitute for the events: a renderer that polls this every
/// frame instead of handling
/// [`ShellEvent::Resized`](crate::ShellEvent::Resized) will recreate its
/// swapchain a frame late.
///
/// # Requested versus effective
///
/// The split down the middle of this struct is the seam's answer to "a window
/// request is not a window command":
///
/// * [`requested_mode`](Self::requested_mode) and
///   [`requested_constraints`](Self::requested_constraints) are what *we asked
///   for*. They are always known, because we said them.
/// * [`configuration`](Self::configuration) is what the *window system did*.
///   It is `None` until the first configure arrives, and its
///   [`mode`](WindowConfiguration::mode) can disagree with the requested one
///   forever.
///
/// Nothing here reports "effective constraints", because no platform has such a
/// concept: min/max/aspect are hints whose only observable effect is the size
/// in [`configuration`](Self::configuration).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowState {
    /// What the window system has told us, or `None` if it has not yet.
    ///
    /// See [`WindowState::size`] for the ordering constraint this creates.
    pub configuration: Option<WindowConfiguration>,
    /// The mode most recently asked for through
    /// [`Shell::set_mode`](crate::Shell::set_mode) or
    /// [`WindowDesc::mode`].
    pub requested_mode: DisplayMode,
    /// The constraints most recently asked for. Hints; see
    /// [`SizeConstraints`].
    pub requested_constraints: SizeConstraints,
    /// Whether this window has keyboard focus.
    pub focused: bool,
    /// Whether the window is mapped and not minimized.
    pub visible: bool,
    /// Current pointer mode; see [`PointerMode`](crate::PointerMode).
    pub pointer_mode: crate::PointerMode,
    /// Whether a [`CloseRequested`](crate::ShellEvent::CloseRequested) is
    /// awaiting a [`reply_close_request`](crate::Shell::reply_close_request).
    pub close_pending: bool,
}

impl WindowState {
    /// The client area in device pixels, or `None` while the window is
    /// unconfigured.
    ///
    /// # This is the swapchain ordering constraint
    ///
    /// A freshly created window **has no size**. On Wayland an `xdg_surface`
    /// is not sized — and must not be given a buffer — until the compositor
    /// answers with a `configure`, which is a round trip away. A swapchain
    /// needs an extent, so the mandatory sequence is:
    ///
    /// ```text
    /// let window = shell.create_window(&desc)?;
    /// loop {
    ///     shell.pump(&mut |event| { … });          // ← the first Resized
    ///     if let Some(size) = shell.window_state(window)?.size() {
    ///         create_swapchain(size);              // only now
    ///         break;
    ///     }
    /// }
    /// ```
    ///
    /// [`HeadlessShell`](crate::HeadlessShell) deliberately defers the first
    /// configure by at least one `pump` so that this sequence is exercised
    /// rather than merely documented — a test double that succeeds immediately
    /// proves nothing about the code path that matters.
    #[inline]
    #[must_use]
    pub fn size(&self) -> Option<PhysicalSize> {
        self.configuration.map(|config| config.size)
    }

    /// Device pixels per logical unit, or `None` while the window is
    /// unconfigured.
    #[inline]
    #[must_use]
    pub fn scale_factor(&self) -> Option<f64> {
        self.configuration.map(|config| config.scale_factor)
    }

    /// The mode actually in effect, or `None` while the window is unconfigured.
    ///
    /// Compare [`requested_mode`](Self::requested_mode) — they can differ.
    #[inline]
    #[must_use]
    pub fn effective_mode(&self) -> Option<DisplayMode> {
        self.configuration.map(|config| config.mode)
    }

    /// Whether the window system has reported a size yet.
    #[inline]
    #[must_use]
    pub const fn is_configured(&self) -> bool {
        self.configuration.is_some()
    }

    /// Whether the window system honoured the last mode request.
    ///
    /// `false` while unconfigured (nothing has been honoured yet) and `false`
    /// when a compositor refused. A UI showing a fullscreen toggle reads this,
    /// not [`requested_mode`](Self::requested_mode), or the checkbox lies.
    #[must_use]
    pub fn mode_request_honoured(&self) -> bool {
        self.effective_mode() == Some(self.requested_mode)
    }
}

/// What to do about a close request.
///
/// The reply exists because closing is the one window operation with a
/// *decision* in it — "you have unsaved changes" — and a shell that closes the
/// window before asking has already lost the argument. Both Wayland
/// (`xdg_toplevel.close` is purely advisory) and X11
/// (`WM_DELETE_WINDOW`) model it exactly this way; Win32's `WM_CLOSE` does too.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CloseReply {
    /// Destroy the window. A
    /// [`WindowDestroyed`](crate::ShellEvent::WindowDestroyed) event follows.
    #[default]
    Close,
    /// Keep it open — the application handled the request itself, by showing a
    /// save prompt or by refusing.
    Keep,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_is_no_true_fullscreen_variant() {
        // The assertion is structural: `DisplayMode` has two variants and this
        // match is exhaustive without a wildcard. Adding exclusive fullscreen
        // would break this file, which is the intent — see the module docs.
        let modes = [
            DisplayMode::Windowed,
            DisplayMode::Borderless { monitor: None },
        ];
        for mode in modes {
            match mode {
                DisplayMode::Windowed => assert!(!mode.is_borderless()),
                DisplayMode::Borderless { .. } => assert!(mode.is_borderless()),
            }
        }
        assert_eq!(DisplayMode::default(), DisplayMode::Windowed);
        assert_eq!(DisplayMode::Windowed.to_string(), "windowed");
        assert_eq!(
            DisplayMode::Borderless { monitor: None }.to_string(),
            "borderless"
        );
        assert_eq!(
            DisplayMode::Borderless {
                monitor: Some(MonitorId(3))
            }
            .to_string(),
            "borderless on monitor 3"
        );
    }

    #[test]
    fn constraints_clamp_then_fit() {
        let constraints = SizeConstraints {
            min: Some(LogicalSize::new(320.0, 240.0)),
            max: Some(LogicalSize::new(1920.0, 1080.0)),
            aspect: None,
        };
        assert_eq!(
            constraints.apply(PhysicalSize::new(100, 100), 1.0),
            PhysicalSize::new(320, 240)
        );
        assert_eq!(
            constraints.apply(PhysicalSize::new(4000, 4000), 1.0),
            PhysicalSize::new(1920, 1080)
        );
        // Constraints are logical, so they scale with the monitor.
        assert_eq!(
            constraints.apply(PhysicalSize::new(100, 100), 2.0),
            PhysicalSize::new(640, 480)
        );
    }

    #[test]
    fn an_aspect_lock_wins_against_the_minimum() {
        // The documented tie-break: a wrong-shaped window is visibly broken, a
        // slightly-too-small one is not.
        let constraints = SizeConstraints::min(LogicalSize::new(640.0, 640.0))
            .with_aspect(AspectRatio::WIDESCREEN);
        let result = constraints.apply(PhysicalSize::new(100, 100), 1.0);
        assert_eq!(result, PhysicalSize::new(640, 360));
        assert!(result.height < 640, "the aspect fit took precedence");
    }

    #[test]
    fn no_constraints_is_the_identity() {
        let size = PhysicalSize::new(1234, 567);
        assert_eq!(SizeConstraints::NONE.apply(size, 1.0), size);
        assert_eq!(SizeConstraints::default(), SizeConstraints::NONE);
    }

    #[test]
    fn an_unconfigured_window_has_no_size_at_all() {
        let unconfigured = WindowState {
            configuration: None,
            requested_mode: DisplayMode::Borderless { monitor: None },
            requested_constraints: SizeConstraints::NONE,
            focused: false,
            visible: true,
            pointer_mode: crate::PointerMode::Free,
            close_pending: false,
        };
        assert!(!unconfigured.is_configured());
        assert_eq!(unconfigured.size(), None);
        assert_eq!(unconfigured.scale_factor(), None);
        assert_eq!(unconfigured.effective_mode(), None);
        assert!(
            !unconfigured.mode_request_honoured(),
            "nothing has been honoured before the first configure"
        );
    }

    #[test]
    fn a_refused_mode_request_is_visible_as_a_disagreement() {
        // The compositor said no. `requested_mode` still says what we asked;
        // the configuration says what happened. A UI toggle must read the
        // latter.
        let refused = WindowState {
            configuration: Some(WindowConfiguration {
                size: PhysicalSize::new(1280, 720),
                scale_factor: 1.0,
                mode: DisplayMode::Windowed,
            }),
            requested_mode: DisplayMode::Borderless { monitor: None },
            requested_constraints: SizeConstraints::NONE,
            focused: true,
            visible: true,
            pointer_mode: crate::PointerMode::Free,
            close_pending: false,
        };
        assert!(refused.is_configured());
        assert_eq!(refused.size(), Some(PhysicalSize::new(1280, 720)));
        assert_eq!(refused.effective_mode(), Some(DisplayMode::Windowed));
        assert!(!refused.mode_request_honoured());

        let honoured = WindowState {
            requested_mode: DisplayMode::Windowed,
            ..refused
        };
        assert!(honoured.mode_request_honoured());
    }

    #[test]
    fn the_default_window_is_a_sane_720p_one() {
        let desc = WindowDesc::default();
        assert_eq!(desc.size, LogicalSize::new(1280.0, 720.0));
        assert_eq!(desc.mode, DisplayMode::Windowed);
        assert!(desc.resizable && desc.visible);
        assert!(!desc.accept_drops, "drops are opt-in");
        assert!(!desc.app_id.is_empty(), "app_id has no useful empty value");
        assert_eq!(CloseReply::default(), CloseReply::Close);
    }
}
