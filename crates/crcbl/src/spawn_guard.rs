//! One lock between the children this test binary starts and the tests that
//! let a file lock go and expect it free again.
//!
//! A child starts with a copy of every descriptor this process has open, and
//! a file opened `O_CLOEXEC` — every file std opens — is closed in it only
//! when it execs. A `flock` belongs to the open file, so a lock let go while
//! a child sits between the two is still held, by that child, and the next
//! take of it here is refused. [`Document`]'s scene lock and the recovery
//! copies' in-use marker are both such locks, and both are let go inside
//! calls a test cannot wrap (a save-as, a new scene, the marker's own probe),
//! so a test that expects one free again holds [`no_spawns`] for its whole
//! body rather than around a drop. On Windows std opens files with handles no
//! child inherits, so there the lock costs only the wait.
//!
//! [`Document`]: crate::scene_edit::Document

use std::process::{Command, Output, Stdio};
use std::sync::{PoisonError, RwLock};

/// Read across every child [`output`] starts, and written by [`no_spawns`].
static SPAWNING: RwLock<()> = RwLock::new(());

/// [`Command::output`], started while no test holds [`no_spawns`]. Returns
/// once the child has exited; its start returned once it had exec'd, as
/// std's spawn does on Linux and macOS (by `posix_spawn`, or by forking and
/// waiting for the exec), so the files it was handed are closed in it by
/// the time the guard goes.
///
/// # Errors
///
/// As [`Command::output`].
pub(crate) fn output(command: &mut Command) -> std::io::Result<Output> {
    let child = {
        let _spawning = SPAWNING.read().unwrap_or_else(PoisonError::into_inner);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?
    };
    child.wait_with_output()
}

/// Holds off every [`output`] until the guard drops, once each one already
/// started has exec'd — what a test that lets a file lock go and takes it
/// again holds; see the module docs.
#[cfg(feature = "scene")]
pub(crate) fn no_spawns() -> std::sync::RwLockWriteGuard<'static, ()> {
    SPAWNING.write().unwrap_or_else(PoisonError::into_inner)
}
