//! Window bring-up: opening a window and waiting for the size a swapchain
//! needs.

use std::time::Duration;

use crcbl_core::time::{MonotonicTime, TimeSource};
use crcbl_shell::{Shell, ShellError, ShellEvent, WindowId};

/// How long to wait for the window to configure before giving up.
pub const CONFIGURE_TIMEOUT: Duration = Duration::from_secs(10);

/// Why [`wait_for_configure`] gave up.
#[derive(Debug)]
pub enum ConfigureError {
    /// The shell refused a call.
    Shell(ShellError),
    /// No configure arrived within [`CONFIGURE_TIMEOUT`].
    TimedOut,
}

impl std::fmt::Display for ConfigureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Shell(error) => write!(f, "shell error: {error}"),
            Self::TimedOut => write!(
                f,
                "the window system never configured the window within {}s, \
                 so it never had a size",
                CONFIGURE_TIMEOUT.as_secs()
            ),
        }
    }
}

impl std::error::Error for ConfigureError {}

impl From<ShellError> for ConfigureError {
    fn from(error: ShellError) -> Self {
        Self::Shell(error)
    }
}

/// Logs the backend, puts the shell's event clock on the caller's, and creates
/// the window.
///
/// The three lines every sample runs before the native and browser paths
/// diverge over *how they wait* for the first configure. `desc` stays the
/// caller's, because a title, an app id and a preferred size are the game's to
/// choose and nothing here could guess them.
///
/// **The clock alignment is the load-bearing line**, and it is the one a game
/// writing this by hand would omit: shell events carry their own timestamps, and
/// a shell whose clock never met the caller's reports every event at an origin
/// the loop cannot compare against its own. `elapsed` is how long the caller's
/// clock has been running; `crcbl::engine::open_window` passes the engine
/// `Clock`'s.
///
/// # Errors
///
/// [`ShellError`] if the shell refused the window.
pub fn open_window<S: Shell + ?Sized>(
    shell: &mut S,
    elapsed: Duration,
    desc: &crcbl_shell::WindowDesc<'_>,
) -> Result<WindowId, ShellError> {
    log::info!(
        "shell: {} backend, caps {:?}",
        shell.backend(),
        shell.caps()
    );
    shell.align_event_clock(elapsed);
    shell.create_window(desc)
}

/// Reports the size a window first configured at.
///
/// **One place, because two paths report it and only one of them is checked.**
/// The browser gate asserts this line by its exact text — `web/tools/
/// browser-e2e.mjs` matches "the shell reported the canvas size" against it —
/// and the browser reaches it through `crcbl::engine::PolledBoot`, never
/// through [`wait_for_configure`]. Every native sample used to carry its own
/// copy of the `info!`, so the six that the gate does not run could have
/// drifted from the one it does and nothing would have said so.
///
/// Public only so `PolledBoot` in the umbrella can say the same line; not part
/// of this crate's API.
#[doc(hidden)]
pub fn log_first_configure(extent: (u32, u32)) {
    log::info!("shell: first configure at {}x{}", extent.0, extent.1);
}

/// Pumps until `window` reports a size, and returns it.
///
/// A swapchain needs a size and an unconfigured window does not have one, so
/// this is the gate between [`Shell::create_window`] and opening a device on
/// the window ([`Presenter::request_open`](crate::present::Presenter::request_open)).
///
/// **Every event pumped while waiting goes to `sink`**, as
/// [`Shell::pump`] hands them over. What arrives before the first size is not
/// noise to discard: a [`FocusRefused`](crcbl_shell::ShellEvent::FocusRefused)
/// for a launch the system would not bring forward, a
/// [`ScaleFactorChanged`](crcbl_shell::ShellEvent::ScaleFactorChanged), a key
/// typed while the window was mapping. A program that sizes first and then
/// runs its loop queues them for that loop; one that only wants a count can
/// count.
///
/// # Errors
///
/// [`ConfigureError`] if the shell failed, or if no configure arrived within
/// [`CONFIGURE_TIMEOUT`].
pub fn wait_for_configure<S: Shell + ?Sized>(
    shell: &mut S,
    window: WindowId,
    sink: &mut dyn FnMut(ShellEvent),
) -> Result<(u32, u32), ConfigureError> {
    let started = MonotonicTime::new();
    loop {
        shell.pump(&mut |event| {
            log::debug!("shell event: {event:?}");
            sink(event);
        });
        if let Some(size) = shell.window_state(window)?.size() {
            let extent = (size.width, size.height);
            log_first_configure(extent);
            return Ok(extent);
        }
        if started.elapsed() >= CONFIGURE_TIMEOUT {
            return Err(ConfigureError::TimedOut);
        }
        shell.wait_events(Some(Duration::from_millis(8)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_shell::{HeadlessShell, WindowDesc};

    /// **What arrives before the first size reaches the caller**, in order,
    /// rather than being counted and dropped: the refused launch a launcher
    /// warns about is exactly such an event.
    #[test]
    fn events_pumped_before_the_first_size_reach_the_sink() {
        /// Pumps the configure is held back for, so the wait loops.
        const DELAY: u32 = 3;

        let mut shell = HeadlessShell::new().with_configure_delay(DELAY);
        let window = shell
            .create_window(&WindowDesc::default())
            .expect("a headless window");
        shell.inject(ShellEvent::FocusRefused { window });
        let mut seen = Vec::new();
        let extent =
            wait_for_configure(&mut shell, window, &mut |event| seen.push(event)).expect("sized");
        assert!(extent.0 > 0 && extent.1 > 0, "a size: {extent:?}");
        assert!(
            seen.iter().any(
                |event| matches!(event, ShellEvent::FocusRefused { window: w } if *w == window)
            ),
            "the refusal reached the sink: {seen:?}"
        );
        assert!(
            shell.wait_count() > 0,
            "the configure was held back, so the wait looped"
        );
    }
}
