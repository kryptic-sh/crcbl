//! Windows: WinRT's `UserNotificationListener`, which reads and removes the
//! toasts other apps have shown.
//!
//! The listener is reached with no package identity. Its access status is
//! read, never requested: `RequestAccessAsync` has to be called from a UI
//! thread, and a store that has not been allowed answers
//! [`NotificationError::Undecided`] instead.

use std::time::{Duration, SystemTime};

use windows::Foundation::DateTime;
use windows::UI::Notifications::Management::{
    UserNotificationListener, UserNotificationListenerAccessStatus,
};
use windows::UI::Notifications::{NotificationKinds, UserNotification};

use crate::{
    Notification, NotificationCaps, NotificationError, NotificationId, NotifyingApp, title_and_body,
};

/// The listener, which WinRT makes agile: usable from any thread.
#[derive(Debug)]
pub(crate) struct Backend {
    listener: UserNotificationListener,
}

pub(crate) fn open() -> Result<Backend, NotificationError> {
    let listener =
        UserNotificationListener::Current().map_err(failed("opening the notification listener"))?;
    let status = listener
        .GetAccessStatus()
        .map_err(failed("reading notification access"))?;
    match status {
        UserNotificationListenerAccessStatus::Allowed => Ok(Backend { listener }),
        UserNotificationListenerAccessStatus::Denied => Err(NotificationError::Denied),
        _ => Err(NotificationError::Undecided),
    }
}

impl Backend {
    pub(crate) fn caps(&self) -> NotificationCaps {
        NotificationCaps::READ | NotificationCaps::DISMISS
    }

    pub(crate) fn list(&self) -> Result<Vec<Notification>, NotificationError> {
        let shown = self
            .listener
            .GetNotificationsAsync(NotificationKinds::Toast)
            .and_then(|operation| operation.join())
            .map_err(failed("listing notifications"))?;
        shown
            .into_iter()
            .map(|notification| read(&notification).map_err(failed("reading a notification")))
            .collect()
    }

    pub(crate) fn dismiss(&self, id: NotificationId) -> Result<(), NotificationError> {
        self.listener
            .RemoveNotification(id.0)
            .map_err(failed("dismissing a notification"))
    }
}

/// One listed toast, as the crate describes it.
fn read(notification: &UserNotification) -> windows::core::Result<Notification> {
    let app = notification.AppInfo()?;
    let mut lines = Vec::new();
    for binding in notification.Notification()?.Visual()?.Bindings()? {
        for text in binding.GetTextElements()? {
            lines.push(text.Text()?.to_string());
        }
    }
    let (title, body) = title_and_body(lines);
    Ok(Notification {
        id: NotificationId(notification.Id()?),
        app: NotifyingApp {
            id: app.AppUserModelId()?.to_string(),
            name: app.DisplayInfo()?.DisplayName()?.to_string(),
        },
        arrived: from_date_time(notification.CreationTime()?),
        title,
        body,
    })
}

/// WinRT's `DateTime` epoch, 1601-01-01 UTC, from the Unix epoch, in its own
/// 100-nanosecond ticks: a fixed constant of the two calendars.
const TICKS_BEFORE_UNIX_EPOCH: i64 = 116_444_736_000_000_000;

/// How many WinRT ticks make a nanosecond's hundred.
const NANOS_PER_TICK: u64 = 100;

/// The moment `time` names.
fn from_date_time(time: DateTime) -> SystemTime {
    let since_unix = time.UniversalTime - TICKS_BEFORE_UNIX_EPOCH;
    let length = Duration::from_nanos(since_unix.unsigned_abs() * NANOS_PER_TICK);
    if since_unix >= 0 {
        SystemTime::UNIX_EPOCH + length
    } else {
        SystemTime::UNIX_EPOCH - length
    }
}

/// A failed WinRT call, as the crate reports it.
fn failed(what: &'static str) -> impl Fn(windows::core::Error) -> NotificationError {
    move |error| NotificationError::Platform {
        what,
        message: error.message(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A `DateTime` is the moment it names**: the Unix epoch, a second
    /// either side of it, and a known date, each from its tick count.
    #[test]
    fn a_date_time_is_the_moment_it_names() {
        let at = |ticks: i64| {
            from_date_time(DateTime {
                UniversalTime: ticks,
            })
        };
        assert_eq!(at(TICKS_BEFORE_UNIX_EPOCH), SystemTime::UNIX_EPOCH);
        assert_eq!(
            at(TICKS_BEFORE_UNIX_EPOCH + 10_000_000),
            SystemTime::UNIX_EPOCH + Duration::from_secs(1)
        );
        assert_eq!(
            at(TICKS_BEFORE_UNIX_EPOCH - 10_000_000),
            SystemTime::UNIX_EPOCH - Duration::from_secs(1)
        );
        // 2026-10-30T00:00:00Z, 1_793_318_400 s after the Unix epoch.
        assert_eq!(
            at(TICKS_BEFORE_UNIX_EPOCH + 17_933_184_000_000_000),
            SystemTime::UNIX_EPOCH + Duration::from_secs(1_793_318_400)
        );
    }

    /// **This machine's store lists, the same ids twice, and dismisses**,
    /// printing each sender's id and nothing of what a notification says. Ignored: it reads the
    /// user's real notifications, which CI has none of and a run must not
    /// print. Dismissing an id nothing holds is the only removal it makes.
    #[test]
    #[ignore = "reads this machine's real notification store"]
    fn this_machines_store_lists_and_dismisses() {
        let store = crate::Notifications::open().expect("open");
        let listed = store.list().expect("list");
        println!("{} notifications", listed.len());
        for notification in &listed {
            println!("{:?} from {}", notification.id, notification.app.id);
        }
        let ids = |listed: &[crate::Notification]| {
            listed
                .iter()
                .map(|notification| notification.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            ids(&store.list().expect("list again")),
            ids(&listed),
            "a second snapshot named the same notifications differently"
        );
        store
            .dismiss(NotificationId(u32::MAX))
            .expect("dismissing an id nothing holds");
    }
}
