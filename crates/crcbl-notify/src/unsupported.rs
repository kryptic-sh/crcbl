//! A platform with no notification backend yet: Linux and macOS
//! (`docs/backlog.md`, _A native notification seam_).
//!
//! [`Backend`] has no values, so a [`Notifications`](crate::Notifications)
//! cannot exist here: [`open`] is the one function that answers, and it
//! answers [`NotificationError::Unsupported`]. Every other method is a `match`
//! on nothing.

use crate::{Notification, NotificationCaps, NotificationError, NotificationId};

/// No backend: uninhabited.
#[derive(Debug)]
pub(crate) enum Backend {}

pub(crate) fn open() -> Result<Backend, NotificationError> {
    Err(NotificationError::Unsupported(
        "reading notifications has a Windows backend only so far",
    ))
}

impl Backend {
    pub(crate) fn caps(&self) -> NotificationCaps {
        match *self {}
    }

    pub(crate) fn list(&self) -> Result<Vec<Notification>, NotificationError> {
        match *self {}
    }

    pub(crate) fn dismiss(&self, _id: NotificationId) -> Result<(), NotificationError> {
        match *self {}
    }
}
