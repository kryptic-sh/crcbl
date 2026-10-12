//! A [`Command`] for a child process that opens no console window.
//!
//! On Windows a console program started by a process that has no console of
//! its own — `cargo` run from an agent's shell, a test harness, an IDE — is
//! given a new console, and that console has a window: one flashes up for
//! every build-script compiler call and every test that re-runs itself. A
//! process that does have a console passes it on and nothing appears.
//!
//! [`command`] is [`Command::new`] that marks the child `CREATE_NO_WINDOW`
//! when this process has no console, and leaves it alone when it has one, so
//! a developer running `crcbl run` in a terminal still sees the child's
//! output there. Piped and captured output, and exit codes, are unaffected
//! either way. Other platforms never open a window for a child, so there it
//! is exactly [`Command::new`].
//!
//! The workspace's `clippy.toml` disallows [`Command::new`], so a new spawn
//! cannot forget this.
//!
//! Verified on Windows only by the run that found the windows, and by this
//! crate's test; Linux and macOS take the plain path.

use std::ffi::OsStr;
use std::process::Command;

/// [`Command::new`], opening no console window for the child on Windows.
#[must_use]
pub fn command(program: impl AsRef<OsStr>) -> Command {
    #[expect(
        clippy::disallowed_methods,
        reason = "the one place the plain constructor is wrapped"
    )]
    let mut command = Command::new(program);
    windowless(&mut command);
    command
}

/// Marks `command`'s child `CREATE_NO_WINDOW` if this process has no console.
#[cfg(windows)]
fn windowless(command: &mut Command) {
    use std::os::windows::process::CommandExt as _;

    /// `CREATE_NO_WINDOW` from the process creation flags; fixed by the
    /// Win32 ABI.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    if !has_console() {
        command.creation_flags(CREATE_NO_WINDOW);
    }
}

#[cfg(not(windows))]
const fn windowless(_: &mut Command) {}

/// Whether this process is attached to a console, with a window or without:
/// a child of a windowless console shares it and shows nothing, and must keep
/// it, or output written to the inherited console handles is lost.
#[cfg(windows)]
fn has_console() -> bool {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        // `DWORD GetConsoleProcessList(LPDWORD, DWORD)`: the processes on this
        // process's console, zero when it has none.
        fn GetConsoleProcessList(list: *mut u32, count: u32) -> u32;
    }
    let mut list = [0_u32; 1];
    // SAFETY: a one-element buffer and its true length; the call writes at
    // most that many ids.
    unsafe { GetConsoleProcessList(list.as_mut_ptr(), 1) != 0 }
}

#[cfg(test)]
mod tests {
    use super::command;

    /// Set in the child [`a_child_opens_a_console_window_only_with_its_parent`] starts.
    #[cfg(windows)]
    const PROBE: &str = "CRCBL_SPAWN_PROBE";

    /// The child still runs, and its exit code and captured output come back.
    #[test]
    fn a_child_runs_and_its_output_is_captured() {
        let output = command(env!("CARGO"))
            .arg("--version")
            .output()
            .expect("cargo runs");
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).starts_with("cargo "));
    }

    /// Runs this test binary's probe test as a child made by `command`, with
    /// `mode` in [`PROBE`], and returns its stdout.
    #[cfg(windows)]
    fn probe(mode: &str, flags: u32) -> String {
        use std::os::windows::process::CommandExt as _;
        let mut child = command(std::env::current_exe().expect("a test binary knows its path"));
        child
            .args([
                "--exact",
                "tests::a_child_opens_a_console_window_only_with_its_parent",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(PROBE, mode);
        if flags != 0 {
            child.creation_flags(flags);
        }
        let output = child.output().expect("the probe runs");
        assert!(output.status.success(), "{output:?}");
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    /// Whether this process's console has a window: null from
    /// `GetConsoleWindow` for no console, and for one made windowless.
    #[cfg(windows)]
    fn console_window() -> bool {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetConsoleWindow() -> *mut core::ffi::c_void;
        }
        // SAFETY: takes no arguments and only reads this process's console.
        !unsafe { GetConsoleWindow() }.is_null()
    }

    /// **A child opens a console window only if its parent's console has
    /// one.** A parent on a console shares it, window or not. A parent with
    /// none — here a middle process started `DETACHED_PROCESS`, so this runs
    /// whatever this machine's own console is — would hand a plain child a
    /// new console with a window; `command`'s child gets a windowless one.
    #[cfg(windows)]
    #[test]
    fn a_child_opens_a_console_window_only_with_its_parent() {
        /// `DETACHED_PROCESS`: the child starts with no console at all.
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        match std::env::var(PROBE).as_deref() {
            Ok("report") => println!("window={}", console_window()),
            Ok("middle") => {
                println!("middle={}", super::has_console());
                print!("{}", probe("report", 0));
            }
            _ => {
                let here = probe("report", 0);
                let expected = format!("window={}", console_window());
                assert!(here.contains(&expected), "wanted {expected}: {here}");
                let detached = probe("middle", DETACHED_PROCESS);
                assert!(detached.contains("middle=false"), "{detached}");
                assert!(
                    detached.contains("window=false"),
                    "the child of a consoleless process opened a window: {detached}"
                );
            }
        }
    }
}
