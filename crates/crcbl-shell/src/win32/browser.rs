//! Opening a [`WebUrl`] in the user's browser, for
//! [`Shell::open_url`](crate::Shell::open_url): `ShellExecuteW` with the
//! `open` verb.
//!
//! # Only a `WebUrl` reaches the call
//!
//! `ShellExecuteW(open)` is the most general "do something with this string"
//! Windows has: given a path it runs the program or opens the file with its
//! associated application, and given any registered scheme it starts that
//! scheme's handler. Its argument is therefore never a caller's string, only a
//! [`WebUrl`], which is an absolute `http` or `https` URL with no character a
//! command line or a path could read differently; see the [`url`](crate::url)
//! module. What it starts is whatever the user registered for those two
//! schemes, which is the browser.
//!
//! # COM: an apartment for the call, and only for the call
//!
//! The `ShellExecuteW` documentation asks for COM on the calling thread,
//! `CoInitializeEx(COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE)`, because
//! the scheme's handler may be a COM object (a `DelegateExecute` verb) rather
//! than a command line, and some of those need a single-threaded apartment.
//! This backend otherwise never touches COM (`win32::dnd` says why it uses
//! `WM_DROPFILES` and not OLE drag and drop), so it does not make the loop
//! thread an apartment for the shell's lifetime: [`Apartment`] enters one
//! around the call and leaves it after.
//!
//! That is safe against a host that has its own plans for the thread because
//! COM counts: a thread already in a single-threaded apartment answers
//! `S_FALSE` and is only counted once more, and the matching
//! `CoUninitialize` takes the count back to where the host left it. A thread
//! the host put in the multithreaded apartment answers `RPC_E_CHANGED_MODE`;
//! nothing was entered then, nothing is left, and the call goes ahead in the
//! host's apartment, since moving the thread out of it is not this call's to
//! do. A handler that insists on a single-threaded apartment then refuses, and
//! the refusal comes back as an error code like any other.
//!
//! `ShellExecuteW` is synchronous, and a COM handler may run a modal message
//! loop inside it, so this thread's window procedure can run before it
//! returns, exactly as it does inside `SetWindowPos`. Nothing here holds a
//! borrow across the call.

use crate::{ShellError, WebUrl};

use super::ffi::{self, Handle, value};

/// The largest `ShellExecuteW` answer that is an error code; anything above is
/// success.
const LAST_ERROR_CODE: usize = 32;

/// Opens `url` with whatever is registered for its scheme, `owner` owning any
/// message box the system shows on the way.
///
/// # Errors
///
/// [`ShellError::Backend`] naming `ShellExecuteW`'s error code, for example
/// when no browser is registered for the scheme.
pub(super) fn open(owner: Handle, url: &WebUrl) -> Result<(), ShellError> {
    let verb = ffi::wide("open").ok_or_else(|| interior_nul("the verb"))?;
    let file = ffi::wide(url.as_str()).ok_or_else(|| interior_nul("the URL"))?;
    let _apartment = Apartment::enter()?;
    // SAFETY: `verb` and `file` are NUL-terminated UTF-16 buffers that live
    // until the call returns; the parameters and directory may be null, and
    // `owner` is a window of this thread or null. The call keeps no pointer.
    let answer = unsafe {
        ffi::ShellExecuteW(
            owner,
            verb.as_ptr(),
            file.as_ptr(),
            core::ptr::null(),
            core::ptr::null(),
            value::SW_SHOW_NORMAL,
        )
    };
    // The answer is an `HINSTANCE` only for compatibility; its value is all
    // that means anything.
    let code = answer as usize;
    if code > LAST_ERROR_CODE {
        Ok(())
    } else {
        Err(ShellError::Backend(format!(
            "ShellExecuteW could not open {url}: {} ({code})",
            refusal_name(code)
        )))
    }
}

/// A string `ffi::wide` refused. Unreachable for a [`WebUrl`], which cannot
/// hold a NUL, and reported rather than unwrapped all the same.
fn interior_nul(what: &str) -> ShellError {
    ShellError::Backend(format!(
        "{what} holds a NUL, which ShellExecuteW cannot take"
    ))
}

/// The name `shellapi.h` and `winerror.h` give a `ShellExecuteW` error code.
fn refusal_name(code: usize) -> &'static str {
    match code {
        0 => "out of memory or resources",
        2 => "ERROR_FILE_NOT_FOUND",
        3 => "ERROR_PATH_NOT_FOUND",
        5 => "SE_ERR_ACCESSDENIED",
        8 => "SE_ERR_OOM",
        11 => "ERROR_BAD_FORMAT",
        26 => "SE_ERR_SHARE",
        27 => "SE_ERR_ASSOCINCOMPLETE",
        28 => "SE_ERR_DDETIMEOUT",
        29 => "SE_ERR_DDEFAIL",
        30 => "SE_ERR_DDEBUSY",
        31 => "SE_ERR_NOASSOC, no program is registered for the scheme",
        32 => "SE_ERR_DLLNOTFOUND",
        _ => "an undocumented code",
    }
}

/// A COM apartment entered for one call, left when dropped; see the module
/// docs for why that is safe whatever the host did with the thread.
struct Apartment {
    /// Whether `CoInitializeEx` counted this thread in, so a
    /// `CoUninitialize` is owed.
    owes: bool,
}

impl Apartment {
    /// Enters a single-threaded apartment, or stays in the multithreaded one
    /// the thread is already in.
    ///
    /// # Errors
    ///
    /// [`ShellError::Backend`] for any other refusal, such as
    /// `E_OUTOFMEMORY`.
    fn enter() -> Result<Self, ShellError> {
        // SAFETY: a reserved null and a flag word; the call reads no memory of
        // ours and changes only this thread's COM state, which `drop` puts
        // back.
        let result = unsafe {
            ffi::CoInitializeEx(
                core::ptr::null(),
                value::COINIT_APARTMENT_THREADED | value::COINIT_DISABLE_OLE1DDE,
            )
        };
        match result {
            value::S_OK | value::S_FALSE => Ok(Self { owes: true }),
            value::RPC_E_CHANGED_MODE => Ok(Self { owes: false }),
            refused => Err(ShellError::Backend(format!(
                "CoInitializeEx refused the thread before ShellExecuteW: {:#010x}",
                refused.cast_unsigned()
            ))),
        }
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.owes {
            // SAFETY: balances the `CoInitializeEx` that counted this thread
            // in, on the same thread, since `Apartment` lives on one stack
            // frame.
            unsafe { ffi::CoUninitialize() };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `COINIT_MULTITHREADED`, the apartment a host may already have put the
    /// thread in.
    const COINIT_MULTITHREADED: u32 = 0x0;

    /// The apartment is this thread's for the call and is handed back: a
    /// thread that was in none can still be put in the multithreaded one
    /// afterwards, which `CoInitializeEx` refuses with `RPC_E_CHANGED_MODE`
    /// while a single-threaded one is still counted.
    #[test]
    fn the_apartment_is_left_as_it_was_found() {
        std::thread::spawn(|| {
            drop(Apartment::enter().expect("a fresh thread enters"));
            // SAFETY: a reserved null and a flag word, on this thread.
            let multithreaded =
                unsafe { ffi::CoInitializeEx(core::ptr::null(), COINIT_MULTITHREADED) };
            assert_eq!(multithreaded, value::S_OK, "nothing was left behind");
            // A thread in the multithreaded apartment stays there and owes
            // nothing.
            let inside = Apartment::enter().expect("the host's apartment is used");
            assert!(!inside.owes);
            drop(inside);
            // SAFETY: balances the `CoInitializeEx` above, on this thread.
            unsafe { ffi::CoUninitialize() };
        })
        .join()
        .expect("the test thread");
    }

    /// A thread already in a single-threaded apartment is counted once more
    /// and handed back still in it.
    #[test]
    fn a_hosts_apartment_survives_the_call() {
        std::thread::spawn(|| {
            // SAFETY: a reserved null and a flag word, on this thread.
            let host =
                unsafe { ffi::CoInitializeEx(core::ptr::null(), value::COINIT_APARTMENT_THREADED) };
            assert_eq!(host, value::S_OK);
            let inside = Apartment::enter().expect("the host's apartment is joined");
            assert!(inside.owes, "S_FALSE is counted and owes its uninit");
            drop(inside);
            // Still single-threaded: asking for the multithreaded apartment is
            // refused as a change of mode.
            // SAFETY: as above.
            let multithreaded =
                unsafe { ffi::CoInitializeEx(core::ptr::null(), COINIT_MULTITHREADED) };
            assert_eq!(multithreaded, value::RPC_E_CHANGED_MODE);
            // SAFETY: balances the host's `CoInitializeEx`, on this thread.
            unsafe { ffi::CoUninitialize() };
        })
        .join()
        .expect("the test thread");
    }

    #[test]
    fn an_error_code_is_named() {
        assert_eq!(
            refusal_name(31),
            "SE_ERR_NOASSOC, no program is registered for the scheme"
        );
        assert_eq!(refusal_name(2), "ERROR_FILE_NOT_FOUND");
        assert_eq!(refusal_name(17), "an undocumented code");
    }
}
