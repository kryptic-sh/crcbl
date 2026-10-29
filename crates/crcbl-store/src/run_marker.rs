//! What a run that did not close normally leaves for the next one.
//!
//! A release build on Windows has no console, so a panic or a failed run closes
//! the window with nothing on screen; its message is in the log file
//! ([`enable_log_file`](crate::enable_log_file)), which a player has no reason to
//! look for. So a run writes a marker in the app's data folder as it starts
//! ([`begin`]) and deletes it when it ends cleanly ([`finish`]). A run that ends
//! any other way leaves it behind: one ending on an error or a caught panic also
//! writes its message into it ([`record`]), and one that panics on another
//! thread, aborts, hits a driver fault or is killed writes nothing more. The next
//! run takes it ([`take`]) before writing its own, and can tell the player the
//! last run did not close normally and where its log is.
//!
//! Ported from EW, where a forced kill (`taskkill /F` on Windows, `kill -9` on
//! Linux) left the marker and the next launch reported it once, and a clean run
//! left none.
//!
//! **Never fatal.** A data folder that cannot be opened or a marker that cannot
//! be written is a warning through the log, and the run carries on.

use std::path::Path;

use crate::{NativeStorage, StorageSource};

/// The marker's file name in the app's data folder; it holds the run's error,
/// or [`RUNNING`] while the run has said nothing more.
pub const MARKER: &str = "last-run-failed.txt";

/// What the marker holds from [`begin`] until the run records an error or
/// finishes: what the next run reads when this one could say nothing.
pub const RUNNING: &str = "the run did not close normally";

/// Notes that `app`'s run has started, for the next run to find if this one
/// never reaches [`finish`].
pub fn begin(app: &str) {
    record(app, RUNNING);
}

/// Notes that `app`'s run is stopping on `message`, for the next run to report.
///
/// Called on an error the run returns, or on a panic it caught; the message
/// replaces [`RUNNING`]. Failing to write it is logged: the message itself is in
/// the log already.
pub fn record(app: &str, message: &str) {
    match NativeStorage::data(app) {
        Ok(storage) => record_in(&storage, message),
        Err(error) => {
            crcbl_core::log::warn!("run marker: no data folder to note the run in: {error}")
        }
    }
}

/// Clears `app`'s marker: this run closed normally.
pub fn finish(app: &str) {
    match NativeStorage::data(app) {
        Ok(storage) => {
            take_in(&storage);
        }
        Err(error) => crcbl_core::log::warn!("run marker: no data folder to clear: {error}"),
    }
}

/// What `app`'s last run left, clearing it: `Some` with its message when the
/// run did not close normally, `None` when it did or there was no last run.
///
/// Call it before [`begin`], or the marker read is this run's own.
#[must_use]
pub fn take(app: &str) -> Option<String> {
    NativeStorage::data(app)
        .ok()
        .and_then(|storage| take_in(&storage))
}

fn record_in(storage: &impl StorageSource, message: &str) {
    if let Err(error) = storage.write(Path::new(MARKER), message.as_bytes()) {
        crcbl_core::log::warn!("run marker: could not note the run: {error}");
    }
}

fn take_in(storage: &impl StorageSource) -> Option<String> {
    let marker = Path::new(MARKER);
    if !storage.exists(marker) {
        return None;
    }
    // A marker that cannot be read still says the run did not close normally;
    // only what it said is lost.
    let message = storage.read(marker).map_or_else(
        |_| RUNNING.to_string(),
        |bytes| String::from_utf8_lossy(&bytes).into_owned(),
    );
    if let Err(error) = storage.delete(marker) {
        crcbl_core::log::warn!("run marker: could not clear the last run's note: {error}");
    }
    Some(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStorage;

    /// **A run that never finished is reported once, with what it last
    /// said**: [`RUNNING`] when it said nothing more, its message when it
    /// recorded one. A run that finished leaves nothing.
    #[test]
    fn an_unfinished_run_is_reported_once_with_its_last_word() {
        let storage = MemoryStorage::new();
        assert_eq!(take_in(&storage), None, "no last run");

        record_in(&storage, RUNNING);
        assert_eq!(take_in(&storage).as_deref(), Some(RUNNING), "killed");
        assert_eq!(take_in(&storage), None, "reported once");

        record_in(&storage, RUNNING);
        record_in(&storage, "the device was lost");
        assert_eq!(
            take_in(&storage).as_deref(),
            Some("the device was lost"),
            "the error replaces the running note"
        );

        record_in(&storage, RUNNING);
        take_in(&storage);
        assert_eq!(take_in(&storage), None, "a finished run leaves nothing");
    }
}
