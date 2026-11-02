//! Keeping the display on while a window asks, for
//! [`Shell::set_idle_inhibit`](crate::Shell::set_idle_inhibit):
//! `SetThreadExecutionState`.
//!
//! # Per thread, which is the right scope here
//!
//! The execution state belongs to the **calling thread**: it holds until that
//! thread changes it or exits, and it says nothing about any other thread's.
//! That is the right owner for it. [`Win32Shell`](super::Win32Shell) is not
//! `Send` (it holds an `Rc` and raw window handles), so every call into it —
//! [`set_idle_inhibit`](crate::Shell::set_idle_inhibit), `destroy_window` and
//! its `Drop` alike — runs on the thread that opened it, which is the thread
//! that pumps its windows. One thread sets the state and the same thread
//! clears it; a shell on another thread holds its own.
//!
//! # The windows asking are the bookkeeping; the thread's state follows them
//!
//! [`IdleInhibit`] keeps which windows asked, and holds
//! `ES_CONTINUOUS | ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED` while any does,
//! going back to `ES_CONTINUOUS` alone once none does. It calls the system only
//! when that answer changes, so the second window to ask, or the first of two
//! to stop, costs nothing. A refusal (`SetThreadExecutionState` answering zero)
//! is an error: the window's request is not recorded, and the caller hears.

use crate::{ShellError, WindowId};

use super::ffi::{self, value};

/// What the thread holds while any window asks.
const HOLD: u32 = value::ES_CONTINUOUS | value::ES_DISPLAY_REQUIRED | value::ES_SYSTEM_REQUIRED;
/// What it goes back to once none does: nothing held.
const RELEASE: u32 = value::ES_CONTINUOUS;

/// The windows keeping the display on, and whether this thread holds it on.
#[derive(Debug, Default)]
pub(super) struct IdleInhibit {
    /// Each window that asked and has not stopped, once.
    windows: Vec<WindowId>,
    /// Whether the last successful call held the display on. Kept apart from
    /// `windows` so a release that failed is tried again on the next change
    /// and on drop, rather than forgotten.
    held: bool,
}

impl IdleInhibit {
    /// Records that `window` asks, or no longer asks, and holds or releases
    /// the display if that changed whether any window does.
    ///
    /// # Errors
    ///
    /// [`ShellError::Backend`] if the system refused the change, in which
    /// case the window's request is left as it was.
    pub(super) fn set(&mut self, window: WindowId, inhibit: bool) -> Result<(), ShellError> {
        let asking = self.windows.contains(&window);
        if asking == inhibit {
            return Ok(());
        }
        if inhibit {
            self.windows.push(window);
        } else {
            self.windows.retain(|&other| other != window);
        }
        self.sync().inspect_err(|_| {
            if inhibit {
                self.windows.retain(|&other| other != window);
            } else {
                self.windows.push(window);
            }
        })
    }

    /// Ends a destroyed window's request, releasing the display if it was the
    /// last one asking. The window is gone whatever the system answers, so it
    /// is forgotten even then; a release that failed is tried again on the
    /// next change and on drop.
    ///
    /// # Errors
    ///
    /// [`ShellError::Backend`] if the system refused the release.
    pub(super) fn forget(&mut self, window: WindowId) -> Result<(), ShellError> {
        self.windows.retain(|&other| other != window);
        self.sync()
    }

    /// Makes the thread's state match whether any window asks.
    fn sync(&mut self) -> Result<(), ShellError> {
        let want = !self.windows.is_empty();
        if want != self.held {
            set_thread_state(if want { HOLD } else { RELEASE })?;
            self.held = want;
        }
        Ok(())
    }
}

impl Drop for IdleInhibit {
    /// Releases the display if the shell is dropped with a window still
    /// asking: the state outlives the shell on its thread otherwise, and a
    /// host that keeps running would keep the desktop awake with no window to
    /// show for it.
    fn drop(&mut self) {
        self.windows.clear();
        if let Err(error) = self.sync() {
            crcbl_core::log::warn!("releasing the display on dropping the shell: {error}");
        }
    }
}

/// `SetThreadExecutionState(flags)`, answering the state it replaced.
///
/// # Errors
///
/// [`ShellError::Backend`] for a zero answer, which is how the call refuses.
fn set_thread_state(flags: u32) -> Result<u32, ShellError> {
    // SAFETY: a flag word by value; the call reads no memory of ours and
    // changes only this thread's execution state.
    let previous = unsafe { ffi::SetThreadExecutionState(flags) };
    if previous == 0 {
        return Err(ShellError::Backend(format!(
            "SetThreadExecutionState refused {flags:#010x}"
        )));
    }
    Ok(previous)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_core::Pool;

    /// The display and system bits this thread holds right now.
    ///
    /// Read the only way the system offers: set something and look at what it
    /// replaced. The state is put back at once, so reading it changes nothing.
    fn held_now() -> u32 {
        let previous = set_thread_state(RELEASE).expect("reading the state");
        set_thread_state(previous).expect("putting the state back");
        previous & (value::ES_DISPLAY_REQUIRED | value::ES_SYSTEM_REQUIRED)
    }

    const BOTH: u32 = value::ES_DISPLAY_REQUIRED | value::ES_SYSTEM_REQUIRED;

    /// Runs `test` on a thread of its own, so it starts from a thread that has
    /// held nothing — under `--test-threads=1` every test would otherwise share
    /// the harness's.
    fn on_fresh_thread(test: impl FnOnce() + Send + 'static) {
        std::thread::spawn(test).join().expect("the test thread");
    }

    fn two_windows() -> (WindowId, WindowId) {
        let mut pool = Pool::new();
        (pool.insert(()).cast(), pool.insert(()).cast())
    }

    #[test]
    fn the_display_is_held_while_any_window_asks() {
        on_fresh_thread(|| {
            let (a, b) = two_windows();
            let mut idle = IdleInhibit::default();
            assert_eq!(held_now(), 0, "a fresh thread holds nothing");
            idle.set(a, true).expect("a asks");
            assert_eq!(held_now(), BOTH);
            idle.set(a, true).expect("asking twice is asking once");
            idle.set(b, true).expect("b asks");
            idle.set(a, false).expect("a stops");
            assert_eq!(held_now(), BOTH, "b is still asking");
            idle.set(a, false).expect("stopping twice is stopping once");
            idle.set(b, false).expect("b stops");
            assert_eq!(held_now(), 0, "nobody asks");
        });
    }

    #[test]
    fn destroying_the_last_window_asking_releases_the_display() {
        on_fresh_thread(|| {
            let (a, b) = two_windows();
            let mut idle = IdleInhibit::default();
            idle.set(a, true).expect("a asks");
            idle.forget(b).expect("b never asked");
            assert_eq!(held_now(), BOTH);
            idle.forget(a).expect("a is destroyed");
            assert_eq!(held_now(), 0);
            // A destroyed window's id is not still asking, so asking again
            // with another holds anew.
            idle.set(b, true).expect("b asks");
            assert_eq!(held_now(), BOTH);
            idle.set(b, false).expect("b stops");
        });
    }

    #[test]
    fn dropping_the_shell_releases_the_display() {
        on_fresh_thread(|| {
            let (a, _) = two_windows();
            let mut idle = IdleInhibit::default();
            idle.set(a, true).expect("a asks");
            assert_eq!(held_now(), BOTH);
            drop(idle);
            assert_eq!(held_now(), 0);
        });
    }

    #[test]
    fn the_state_is_the_threads_own() {
        on_fresh_thread(|| {
            let (a, _) = two_windows();
            let mut idle = IdleInhibit::default();
            idle.set(a, true).expect("a asks");
            on_fresh_thread(|| assert_eq!(held_now(), 0, "another thread holds nothing"));
            assert_eq!(held_now(), BOTH);
        });
    }
}
