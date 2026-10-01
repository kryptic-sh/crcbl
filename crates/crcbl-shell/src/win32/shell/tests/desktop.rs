//! Exclusive desktop ownership for libtest's concurrent shell fixtures.

use super::Win32Shell;
use std::ops::{Deref, DerefMut};
use std::sync::{Mutex, MutexGuard};

// nextest's windows-desktop group serializes processes; libtest needs the
// same exclusion among threads in its shared process.
static DESKTOP: Mutex<()> = Mutex::new(());

#[derive(Debug)]
pub(super) struct DesktopShell {
    // Fields drop in declaration order: close windows and release cursor state
    // before another fixture takes ownership of the desktop.
    shell: Win32Shell,
    _desktop: MutexGuard<'static, ()>,
}

pub(super) fn shell() -> DesktopShell {
    // A failed test has already reported its panic and dropped its shell.
    // The gate protects no data, so poisoning must not cancel later tests.
    let desktop = DESKTOP.lock().unwrap_or_else(|error| error.into_inner());
    let shell = Win32Shell::open().expect(
        "opening the Win32 shell needs a usable window station; a failure here is the \
         runner's answer to whether it has one",
    );
    DesktopShell {
        shell,
        _desktop: desktop,
    }
}

impl Deref for DesktopShell {
    type Target = Win32Shell;

    fn deref(&self) -> &Self::Target {
        &self.shell
    }
}

impl DerefMut for DesktopShell {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.shell
    }
}

#[test]
fn the_fixture_holds_the_desktop_gate() {
    let _shell = shell();
    assert!(matches!(
        DESKTOP.try_lock(),
        Err(std::sync::TryLockError::WouldBlock)
    ));
}
