//! Errors crossing the shell seam.
//!
//! One enum, unlike `crcbl-hal`'s two. The HAL splits `SurfaceError` out
//! because `OutOfDate` is *routine* traffic that the frame loop must handle
//! every resize; the shell has no equivalent — nothing here happens on a normal
//! frame. Every variant below means a caller made a mistake, or the window
//! system went away.
//!
//! No platform type appears in any variant. Where a backend has detail worth
//! keeping — a compositor's protocol error string, an X11 error code's name —
//! it goes into a `String`, for the same reason `crcbl-hal` does it: the
//! alternative is losing it or leaking `xcb_generic_error_t` into every caller.

use crate::{ShellBackend, WindowId};

/// A shell operation failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ShellError {
    /// No backend could be opened.
    ///
    /// `tried` lists the backends that were attempted in order, so the log line
    /// distinguishes "there is no `WAYLAND_DISPLAY` and no `DISPLAY`" from "the
    /// Wayland backend is not compiled into this build". See
    /// [`open`](crate::open).
    #[error("no shell backend available (tried: {tried})")]
    NoBackend {
        /// Comma-separated backend names, in the order they were attempted.
        tried: String,
    },

    /// `CRCBL_SHELL` named a backend this build does not have.
    ///
    /// A hard error rather than a fallback: someone who set the variable wanted
    /// that backend, and silently running on a different one turns a debugging
    /// session into a wild goose chase.
    #[error("CRCBL_SHELL={requested:?} is not a backend in this build (have: {available})")]
    UnknownBackend {
        /// The value of `CRCBL_SHELL`.
        requested: String,
        /// Comma-separated names of the backends that are compiled in.
        available: String,
    },

    /// Connecting to the window system failed — no compositor socket, a refused
    /// X11 connection, a browser with no canvas.
    #[error("cannot connect to the {backend} display server: {detail}")]
    Connect {
        /// Backend that failed to connect.
        backend: ShellBackend,
        /// What went wrong, verbatim from the backend.
        detail: String,
    },

    /// The connection to the window system was lost mid-session. Nothing the
    /// shell owns is usable afterwards.
    ///
    /// Distinct from [`ShellError::Connect`] because the recovery differs: a
    /// failed connect is a startup problem a user can fix, and a lost
    /// connection means the compositor died or the session ended, which is a
    /// clean-shutdown signal.
    #[error("{backend} display server connection lost: {detail}")]
    Disconnected {
        /// Backend whose connection dropped.
        backend: ShellBackend,
        /// What the backend reported.
        detail: String,
    },

    /// A [`WindowId`] did not resolve: it was destroyed, never created, or came
    /// from a different shell.
    ///
    /// The generational [`Handle`](crcbl_core::Handle) behind `WindowId` makes
    /// this a clean failure rather than an operation applied to whatever window
    /// took the slot — the same argument as `crcbl-hal`'s `InvalidHandle`.
    #[error("invalid or stale window handle {bits:#x}")]
    InvalidWindow {
        /// The handle's packed bits, from
        /// [`Handle::to_bits`](crcbl_core::Handle::to_bits).
        bits: u64,
    },

    /// A [`MonitorId`](crate::MonitorId) named a monitor that is not connected —
    /// usually one that was unplugged between
    /// [`monitors`](crate::Shell::monitors) and the call that used its id.
    #[error("no such monitor: {0}")]
    NoSuchMonitor(u32),

    /// Creating the window failed.
    #[error("window creation failed: {0}")]
    WindowCreation(String),

    /// A descriptor field was out of range or self-inconsistent — a caller bug,
    /// which is why the message is expected to name the field.
    #[error("invalid descriptor: {0}")]
    InvalidDescriptor(String),

    /// The reply named a window with no close request outstanding.
    ///
    /// An error rather than a no-op because it is always a state-machine bug in
    /// the caller: the reply either arrived twice or was addressed to the wrong
    /// window, and both silently leave a real close request unanswered.
    #[error("window {window:?} has no outstanding close request")]
    NoPendingCloseRequest {
        /// The window the reply named.
        window: WindowId,
    },

    /// The operation is real but this backend cannot do it.
    ///
    /// Always paired with a [`ShellCaps`](crate::ShellCaps) flag: a caller that
    /// checks caps first never sees this, and a caller that does not gets an
    /// error naming the capability it should have checked. Getting this back is
    /// the signal that a consumer is missing a caps branch — which is exactly
    /// the platform-sniffing regression `ShellCaps` exists to prevent.
    #[error("unsupported by the {backend} shell backend: {what}")]
    Unsupported {
        /// Backend that refused.
        backend: ShellBackend,
        /// What was asked for.
        what: &'static str,
    },

    /// Anything else the backend wants to report verbatim.
    #[error("{0}")]
    Backend(String),

    /// The window system requires a recent user interaction for this, and there
    /// has not been one.
    ///
    /// # Why this is a named case rather than a backend string
    ///
    /// It is a **policy**, not a fault, and the policy is the same one on three
    /// platforms:
    ///
    /// * Wayland's `wl_data_device.set_selection` quotes the serial of the
    ///   input event that caused the copy, and a compositor checks it. A client
    ///   that has received no input on a seat cannot take the clipboard.
    /// * A browser gates `navigator.clipboard.write()` and Pointer Lock behind
    ///   a *user gesture*, with the same reasoning and the same failure.
    /// * X11 has no such requirement at all, and neither does Win32.
    ///
    /// What it prevents is a background process silently taking the clipboard —
    /// so a caller must treat it as "ask the user to do that again", never as a
    /// bug to work around. A backend on a platform without the rule **never
    /// returns this**, and that is obviously correct rather than obviously
    /// incomplete.
    #[error("{backend} needs a recent user interaction for {what}")]
    NeedsUserInteraction {
        /// Which backend refused.
        backend: ShellBackend,
        /// What was being attempted, for the log line.
        what: &'static str,
    },
}

impl ShellError {
    /// Builds a [`ShellError::InvalidWindow`] from a window handle.
    ///
    /// Backends call this from their handle-lookup path so the error carries the
    /// same bits that appear in `WindowId`'s `Debug` output.
    #[must_use]
    pub fn invalid_window(window: WindowId) -> Self {
        Self::InvalidWindow {
            bits: window.to_bits(),
        }
    }

    /// Whether the error means the window system is gone and the process should
    /// shut down rather than retry.
    #[must_use]
    pub fn is_fatal(&self) -> bool {
        matches!(self, Self::Disconnected { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_core::Pool;

    #[test]
    fn messages_name_the_specific_problem() {
        let error = ShellError::Unsupported {
            backend: ShellBackend::Wayland,
            what: "pointer warp",
        };
        assert_eq!(
            error.to_string(),
            "unsupported by the wayland shell backend: pointer warp"
        );
        assert!(!error.is_fatal());

        let error = ShellError::NoBackend {
            tried: "wayland, x11".to_string(),
        };
        assert!(error.to_string().contains("wayland, x11"));

        assert!(
            ShellError::Disconnected {
                backend: ShellBackend::X11,
                detail: "server closed the connection".to_string(),
            }
            .is_fatal()
        );
    }

    #[test]
    fn invalid_window_carries_the_handle_bits() {
        let mut pool: Pool<u8> = Pool::new();
        let window: WindowId = pool.insert(0).cast();
        let ShellError::InvalidWindow { bits } = ShellError::invalid_window(window) else {
            panic!("wrong variant");
        };
        assert_eq!(bits, window.to_bits());
        assert_ne!(bits, 0, "packed handle bits are never zero");
    }
}
