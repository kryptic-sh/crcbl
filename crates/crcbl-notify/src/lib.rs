//! The desktop's notifications: read what other apps have shown, and dismiss
//! them.
//!
//! What a bar's notification list does. Any crcbl app can use it; lyr's clock
//! panel is the first. Creating a notification, and the rest of the seam
//! `docs/backlog.md` describes (_A native notification seam_), is not built
//! yet.
//!
//! # What each operation reports
//!
//! [`Notifications::caps`] says which operations this platform performs, one
//! flag each, so "not supported here" is never mistaken for "nothing to show".
//! An operation called without its flag answers
//! [`NotificationError::Unsupported`].
//!
//! # How each platform is reached
//!
//! * **Windows:** WinRT's `UserNotificationListener`, through the `windows`
//!   crate. It reads and dismisses toasts. It needs the user's permission,
//!   which Windows' notification-access privacy setting grants; it does not
//!   need package identity (an unpackaged process was seen allowed on Windows
//!   11, 2026-10-30).
//! * **Linux and macOS:** not built. [`Notifications::open`] reports
//!   [`NotificationError::Unsupported`]. macOS has no public API for another
//!   app's notifications at all.
//!
//! # Polled, not followed
//!
//! [`Notifications::list`] is a snapshot, and there is no change stream: an app
//! calls it again at the rate it chooses and compares the snapshots by
//! [`NotificationId`]. On Windows the listener's change event fails in an
//! unpackaged process (`0x80070490`, "Element not found"), so following would
//! work only for a packaged app.
//!
//! # Threads
//!
//! [`Notifications`] is `Send` and `Sync`, and its calls **block** until the
//! platform answers, so call them from a worker thread rather than a frame
//! loop. The thread needs no setup of its own: on Windows a thread that has not
//! joined a COM apartment is put in the process's multithreaded one by the
//! `windows` crate on its first call.

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use self::windows as platform;
#[cfg(not(windows))]
mod unsupported;
#[cfg(not(windows))]
use unsupported as platform;

use std::time::SystemTime;

bitflags::bitflags! {
    /// The operations this platform performs.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
    pub struct NotificationCaps: u32 {
        /// [`Notifications::list`]: other apps' notifications, read.
        const READ = 1 << 0;
        /// [`Notifications::dismiss`]: another app's notification, removed.
        const DISMISS = 1 << 1;
    }
}

/// A notification's identity, the same in every [`Notifications::list`] for as
/// long as the notification is shown: what a caller compares two snapshots by
/// and dismisses by.
///
/// On Windows it is the listener's `UserNotification.Id`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NotificationId(u32);

/// The app that sent a notification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotifyingApp {
    /// The platform's id for it: on Windows its AppUserModelID, which is what
    /// finds its windows.
    pub id: String,
    /// Its name as the system shows it.
    pub name: String,
}

/// One notification another app has shown.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Notification {
    /// Its identity; see [`NotificationId`].
    pub id: NotificationId,
    /// Who sent it.
    pub app: NotifyingApp,
    /// When it arrived.
    pub arrived: SystemTime,
    /// Its first line of text, empty when it has none.
    pub title: String,
    /// Its other lines of text, joined by newlines.
    pub body: String,
}

/// Why an operation did not happen.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum NotificationError {
    /// This platform does not perform the operation.
    #[error("unsupported here: {0}")]
    Unsupported(&'static str),
    /// The user has refused this app access to notifications.
    #[error("the user has denied access to notifications")]
    Denied,
    /// The user has not decided whether this app may read notifications; on
    /// Windows the notification-access privacy setting decides it.
    #[error("access to notifications has not been granted")]
    Undecided,
    /// A call to the platform failed.
    #[error("{what} failed: {message}")]
    Platform {
        /// What was being done.
        what: &'static str,
        /// The platform's description of the failure.
        message: String,
    },
}

/// The desktop's notification store, opened.
#[derive(Debug)]
pub struct Notifications {
    backend: platform::Backend,
}

impl Notifications {
    /// Opens the store, or says why this app cannot read it.
    pub fn open() -> Result<Self, NotificationError> {
        platform::open().map(|backend| Self { backend })
    }

    /// The operations this platform performs.
    #[must_use]
    pub fn caps(&self) -> NotificationCaps {
        self.backend.caps()
    }

    /// Every notification shown now, oldest first. Blocks until the platform
    /// answers.
    pub fn list(&self) -> Result<Vec<Notification>, NotificationError> {
        let mut listed = self.backend.list()?;
        listed.sort_by_key(|notification| notification.arrived);
        Ok(listed)
    }

    /// Removes the notification `id` names. One already gone is not an error.
    pub fn dismiss(&self, id: NotificationId) -> Result<(), NotificationError> {
        self.backend.dismiss(id)
    }
}

/// A notification's lines of text split as [`Notification`] holds them: the
/// first as its title, and the rest joined by newlines as its body.
#[cfg(any(windows, test))]
fn title_and_body(lines: impl IntoIterator<Item = String>) -> (String, String) {
    let mut lines = lines.into_iter();
    let title = lines.next().unwrap_or_default();
    let body = lines.collect::<Vec<_>>().join("\n");
    (title, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The first line is the title and the rest the body**, in order, with
    /// nothing for a notification that has no text.
    #[test]
    fn the_first_line_is_the_title_and_the_rest_the_body() {
        let lines = |all: &[&str]| all.iter().map(|&line| line.to_owned()).collect::<Vec<_>>();
        assert_eq!(
            title_and_body(lines(&["Build failed", "crcbl", "main"])),
            ("Build failed".to_owned(), "crcbl\nmain".to_owned())
        );
        assert_eq!(
            title_and_body(lines(&["Only a title"])),
            ("Only a title".to_owned(), String::new())
        );
        assert_eq!(title_and_body(lines(&[])), (String::new(), String::new()));
    }

    /// **Every platform but Windows says it cannot open the store**, rather
    /// than opening one that lists nothing.
    #[cfg(not(windows))]
    #[test]
    fn opening_elsewhere_is_unsupported() {
        assert!(matches!(
            Notifications::open(),
            Err(NotificationError::Unsupported(_))
        ));
    }
}
