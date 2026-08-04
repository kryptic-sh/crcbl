//! Reads and writes the desktop clipboard from a process that has never heard
//! of `crcbl-shell`.
//!
//! ```text
//! crcbl-e2e-win32-clip get <format>
//! crcbl-e2e-win32-clip put <format> <text…>
//! ```
//!
//! **Compiled only with the `win32-e2e` feature**, which nothing but
//! `tests/run-win32-e2e.ps1` turns on.
//!
//! # Why a second process is the only honest test
//!
//! The in-crate Windows suite copies with `Win32Shell` and pastes with
//! `Win32Shell`, so it can only ever show that the backend agrees with itself. A
//! clipboard exists to move bytes between *applications*, and the two claims
//! that matter are both about the boundary:
//!
//! * **What we publish is what another application reads.** Not merely "a read
//!   returns what a write wrote" — the format has to be one Windows itself
//!   understands (`CF_UNICODETEXT`, UTF-16 and NUL-terminated) or a registered
//!   format under a name another program can intern by the same string.
//! * **We keep nothing.** `SetClipboardData` gives the memory to the window
//!   station, so the bytes outlive the process that wrote them and are served
//!   with no further conversation. X11 and Wayland both make the writer the
//!   *owner* and have it answer later, which is why both of those backends carry
//!   a transfer state machine and this one does not. A peer that reads our copy
//!   while our own message loop is not running is what tells those two designs
//!   apart, and this program is the peer.
//!
//! # Formats
//!
//! `text` means `CF_UNICODETEXT`, which is what `MimeType::TextUtf8` is
//! published as. Anything else is taken as a registered format *name* and
//! interned with `RegisterClipboardFormatW`, which is how the engine's own
//! mimes reach the clipboard — `application/x-crcbl+ron` is a format called
//! `application/x-crcbl+ron`, no more and no less.
//!
//! # What it prints
//!
//! ```text
//! crcbl-e2e-win32-clip: size <bytes>     (get, when the format is present)
//! crcbl-e2e-win32-clip: text <content>   (get, when the format is present)
//! crcbl-e2e-win32-clip: absent           (get, when it is not)
//! crcbl-e2e-win32-clip: put <bytes>      (put)
//! ```
//!
//! `size` is `GlobalSize`, which Windows is entitled to round up, so it is
//! printed for diagnosis and is not something to assert on. The content is
//! decoded from UTF-16 for `CF_UNICODETEXT` and taken as UTF-8 otherwise, with
//! trailing NULs trimmed either way — the terminator is part of the format, not
//! part of the payload.
//!
//! # Windows only, and it says so out loud
//!
//! `--all-features` turns `win32-e2e` on for every target, so this is built on
//! Linux, macOS and `wasm32` by the lint jobs. They get a `main` that fails and
//! names the reason, for the same reason the key senders do: a helper that
//! reports success on a platform where it cannot have touched a clipboard is
//! worse than one that is missing.

use std::process::ExitCode;

#[cfg(not(target_os = "windows"))]
fn main() -> ExitCode {
    eprintln!(
        "crcbl-e2e-win32-clip: the Win32 clipboard lives in a window station; there is none here"
    );
    ExitCode::FAILURE
}

/// The clipboard surface of `user32` and `kernel32`, hand-written like every
/// other declaration in this crate.
///
/// The peer's own rather than `crcbl_shell::win32::ffi`'s: that module is
/// `pub(crate)`, and a peer built out of the backend's private table would be
/// testing the table against itself.
#[cfg(target_os = "windows")]
mod win32 {
    use core::ffi::c_void;

    /// `HANDLE`/`HGLOBAL`/`HWND`.
    pub type Handle = *mut c_void;

    /// `CF_UNICODETEXT`.
    pub const CF_UNICODETEXT: u32 = 13;
    /// `GMEM_MOVEABLE` — what a clipboard block has to be.
    pub const GMEM_MOVEABLE: u32 = 0x0002;

    #[link(name = "user32")]
    unsafe extern "system" {
        /// Takes the clipboard for this thread. A null window is legal and
        /// associates it with the current task.
        pub fn OpenClipboard(owner: Handle) -> i32;
        /// Gives it back. Every path out of a successful open must reach this.
        pub fn CloseClipboard() -> i32;
        /// Discards the contents; required before writing, and it also makes the
        /// caller the owner.
        pub fn EmptyClipboard() -> i32;
        /// The block published under `format`, still owned by the window
        /// station — never freed by the reader.
        pub fn GetClipboardData(format: u32) -> Handle;
        /// Publishes `data` under `format`, transferring ownership of the block
        /// to the system.
        pub fn SetClipboardData(format: u32, data: Handle) -> Handle;
        /// Whether anything is published under `format`, without opening.
        pub fn IsClipboardFormatAvailable(format: u32) -> i32;
        /// Interns a format name, answering the same id for the same name in
        /// every process on the desktop.
        pub fn RegisterClipboardFormatW(name: *const u16) -> u32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        /// Allocates a moveable block.
        pub fn GlobalAlloc(flags: u32, bytes: usize) -> Handle;
        /// Frees one that was never published.
        pub fn GlobalFree(block: Handle) -> Handle;
        /// Pins a moveable block and answers a pointer to it.
        pub fn GlobalLock(block: Handle) -> *mut c_void;
        /// Releases the pin.
        pub fn GlobalUnlock(block: Handle) -> i32;
        /// The block's size, which may exceed what was asked for.
        pub fn GlobalSize(block: Handle) -> usize;
        /// The calling thread's last error code.
        pub fn GetLastError() -> u32;
    }
}

#[cfg(target_os = "windows")]
fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let (Some(verb), Some(format)) = (args.next(), args.next()) else {
        eprintln!("crcbl-e2e-win32-clip: usage: crcbl-e2e-win32-clip <get|put> <format> [text…]");
        return ExitCode::from(2);
    };
    let format_id = match format_id(&format) {
        Ok(id) => id,
        Err(problem) => {
            eprintln!("crcbl-e2e-win32-clip: {problem}");
            return ExitCode::FAILURE;
        }
    };

    let outcome = match verb.as_str() {
        "get" => get(&format, format_id),
        "put" => {
            let text: Vec<String> = args.collect();
            put(&format, format_id, &text.join(" "))
        }
        other => {
            eprintln!("crcbl-e2e-win32-clip: {other:?} is not get or put");
            return ExitCode::from(2);
        }
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(problem) => {
            eprintln!("crcbl-e2e-win32-clip: {problem}");
            ExitCode::FAILURE
        }
    }
}

/// The clipboard format id a name refers to.
#[cfg(target_os = "windows")]
fn format_id(name: &str) -> Result<u32, String> {
    if name == "text" {
        return Ok(win32::CF_UNICODETEXT);
    }
    let wide: Vec<u16> = name.encode_utf16().chain(core::iter::once(0)).collect();
    // SAFETY: `wide` is a NUL-terminated UTF-16 string that outlives the call,
    // which reads it and retains nothing.
    let id = unsafe { win32::RegisterClipboardFormatW(wide.as_ptr()) };
    if id == 0 {
        // SAFETY: reading the calling thread's last error immediately after the
        // call that set it.
        let error = unsafe { win32::GetLastError() };
        return Err(format!(
            "RegisterClipboardFormatW({name}) failed with Win32 error {error}"
        ));
    }
    Ok(id)
}

/// Reads one format and prints what was there.
#[cfg(target_os = "windows")]
fn get(name: &str, format: u32) -> Result<(), String> {
    // SAFETY: a format id by value; the call only reads window-station state and
    // needs no open clipboard.
    if unsafe { win32::IsClipboardFormatAvailable(format) } == 0 {
        println!("crcbl-e2e-win32-clip: absent");
        return Ok(());
    }

    let clipboard = Clipboard::open()?;
    // SAFETY: the clipboard is open on this thread for as long as `clipboard`
    // lives. The returned block belongs to the window station and must not be
    // freed here; it is only read, and only before the guard closes.
    let block = unsafe { win32::GetClipboardData(format) };
    if block.is_null() {
        // A format that was available a moment ago and is not now: another
        // application emptied the clipboard between the two calls. Reported
        // rather than treated as absence, because they are different findings.
        return Err(format!(
            "{name} was available and GetClipboardData answered null; something else wrote to the \
             clipboard mid-read"
        ));
    }
    // SAFETY: `block` is a live moveable block owned by the window station and
    // the clipboard is open, so it stays valid until the guard drops.
    let size = unsafe { win32::GlobalSize(block) };
    // SAFETY: as above; the lock is released before the guard closes.
    let locked = unsafe { win32::GlobalLock(block) };
    if locked.is_null() {
        return Err(format!("GlobalLock of the {name} block failed"));
    }
    // SAFETY: `locked` points at `size` readable bytes for as long as the lock
    // is held, and the copy is taken before it is released.
    let bytes = unsafe { std::slice::from_raw_parts(locked.cast::<u8>(), size) }.to_vec();
    // SAFETY: releasing the lock taken above, exactly once.
    unsafe { win32::GlobalUnlock(block) };
    drop(clipboard);

    println!("crcbl-e2e-win32-clip: size {size}");
    println!("crcbl-e2e-win32-clip: text {}", decode(format, &bytes));
    Ok(())
}

/// Publishes `text` under one format, and leaves it there.
///
/// The block is *not* freed on success: `SetClipboardData` takes ownership, and
/// the data outliving this process is the property the suite is checking.
#[cfg(target_os = "windows")]
fn put(name: &str, format: u32, text: &str) -> Result<(), String> {
    let payload = encode(format, text);
    // SAFETY: an allocation request by value. `GMEM_MOVEABLE` is what a
    // clipboard block has to be.
    let block = unsafe { win32::GlobalAlloc(win32::GMEM_MOVEABLE, payload.len()) };
    if block.is_null() {
        return Err(format!("GlobalAlloc of {} bytes failed", payload.len()));
    }
    // SAFETY: `block` is a live moveable block this function owns.
    let locked = unsafe { win32::GlobalLock(block) };
    if locked.is_null() {
        // SAFETY: freeing a block this function owns and has not published.
        unsafe { win32::GlobalFree(block) };
        return Err("GlobalLock of a fresh block failed".to_owned());
    }
    // SAFETY: `locked` points at at least `payload.len()` writable bytes, which
    // is the size just requested, and the two regions do not overlap.
    unsafe {
        std::ptr::copy_nonoverlapping(payload.as_ptr(), locked.cast::<u8>(), payload.len());
        win32::GlobalUnlock(block);
    }

    let clipboard = match Clipboard::open() {
        Ok(clipboard) => clipboard,
        Err(problem) => {
            // SAFETY: freeing a block this function still owns, because nothing
            // was published.
            unsafe { win32::GlobalFree(block) };
            return Err(problem);
        }
    };
    // SAFETY: the clipboard is open on this thread. `EmptyClipboard` is required
    // before a write and is what makes this process the owner.
    unsafe { win32::EmptyClipboard() };
    // SAFETY: as above. On success the window station takes ownership of
    // `block`, which is why it is not freed below.
    let published = unsafe { win32::SetClipboardData(format, block) };
    drop(clipboard);
    if published.is_null() {
        // SAFETY: the call failed, so ownership never transferred and this
        // function still owns the block.
        let error = unsafe {
            let error = win32::GetLastError();
            win32::GlobalFree(block);
            error
        };
        return Err(format!(
            "SetClipboardData({name}) failed with Win32 error {error}"
        ));
    }
    println!("crcbl-e2e-win32-clip: put {}", payload.len());
    Ok(())
}

/// `text` as the bytes this format carries, terminator included.
///
/// The terminator is not decoration: `CF_UNICODETEXT` is *defined* as
/// NUL-terminated, and a registered format written the same way is what every
/// application that publishes text under its own name does.
#[cfg(target_os = "windows")]
fn encode(format: u32, text: &str) -> Vec<u8> {
    if format == win32::CF_UNICODETEXT {
        let mut bytes = Vec::with_capacity(text.len() * 2 + 2);
        for unit in text.encode_utf16().chain(core::iter::once(0)) {
            bytes.extend_from_slice(&unit.to_ne_bytes());
        }
        bytes
    } else {
        let mut bytes = text.as_bytes().to_vec();
        bytes.push(0);
        bytes
    }
}

/// The reverse, stopping at the terminator and tolerating padding after it.
#[cfg(target_os = "windows")]
fn decode(format: u32, bytes: &[u8]) -> String {
    if format == win32::CF_UNICODETEXT {
        let mut units = Vec::with_capacity(bytes.len() / 2);
        for pair in bytes.chunks_exact(2) {
            let unit = u16::from_ne_bytes([pair[0], pair[1]]);
            if unit == 0 {
                break;
            }
            units.push(unit);
        }
        String::from_utf16_lossy(&units)
    } else {
        let end = bytes
            .iter()
            .rposition(|byte| *byte != 0)
            .map_or(0, |i| i + 1);
        String::from_utf8_lossy(&bytes[..end]).into_owned()
    }
}

/// An open clipboard that closes on every path out.
///
/// `OpenClipboard` fails while another process holds it, which on Windows is
/// routine rather than exceptional — so this retries with a deadline, the same
/// bounded shape `win32::clipboard`'s own open uses and the same one
/// `docs/plan/12-testing.md` asks for instead of a sleep.
#[cfg(target_os = "windows")]
struct Clipboard;

#[cfg(target_os = "windows")]
impl Clipboard {
    /// How long a contended clipboard is waited out before this gives up.
    const DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);
    /// How long to back off between attempts.
    const BACKOFF: std::time::Duration = std::time::Duration::from_millis(10);

    fn open() -> Result<Self, String> {
        let deadline = std::time::Instant::now() + Self::DEADLINE;
        let mut attempts = 0u32;
        loop {
            attempts += 1;
            // SAFETY: a null owner is documented as legal and associates the
            // clipboard with the current task.
            if unsafe { win32::OpenClipboard(core::ptr::null_mut()) } != 0 {
                return Ok(Self);
            }
            if std::time::Instant::now() >= deadline {
                // SAFETY: reading the calling thread's last error immediately
                // after the call that set it.
                let error = unsafe { win32::GetLastError() };
                return Err(format!(
                    "OpenClipboard failed {attempts} times over {:?}, last Win32 error {error}; \
                     another process is holding the clipboard open",
                    Self::DEADLINE
                ));
            }
            std::thread::sleep(Self::BACKOFF);
        }
    }
}

#[cfg(target_os = "windows")]
impl Drop for Clipboard {
    fn drop(&mut self) {
        // SAFETY: closing a clipboard this guard's construction opened, exactly
        // once. Leaving it open would wedge every other application on the
        // desktop, this suite's next test included.
        unsafe { win32::CloseClipboard() };
    }
}
