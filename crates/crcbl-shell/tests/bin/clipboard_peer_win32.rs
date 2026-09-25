//! Reads and writes the desktop clipboard from a process that has never heard
//! of `crcbl-shell`.
//!
//! ```text
//! crcbl-e2e-win32-clip get <format>
//! crcbl-e2e-win32-clip put <format> <text…>
//! crcbl-e2e-win32-clip put-files <path> [path…]
//! crcbl-e2e-win32-clip put-uri-list-and-files <uri-list> <path> [path…]
//! crcbl-e2e-win32-clip get-files
//! crcbl-e2e-win32-clip hold <ms>
//! ```
//!
//! `hold` keeps the clipboard open for that many milliseconds, which is how the
//! suite makes another process contend for it.
//!
//! `put-files` publishes a `CF_HDROP` naming each path, one argument per path,
//! which is what Explorer's "copy" leaves on the clipboard. The paths need not
//! exist: a file list is names, and nothing on the reading side opens them.
//!
//! `put-uri-list-and-files` publishes a registered `text/uri-list` holding its
//! first argument verbatim *and* a `CF_HDROP` naming the rest, in one write.
//! Every other verb empties the clipboard first, so this is the only way to put
//! two formats with different contents on it at once.
//!
//! `get-files` reads `CF_HDROP` back through `DragQueryFileW`, the way Explorer
//! and every other file-pasting application does, along with the registered
//! `Preferred DropEffect` that tells a paste whether to copy or move.
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
//! crcbl-e2e-win32-clip: absent           (get and get-files, when it is not)
//! crcbl-e2e-win32-clip: put <bytes>      (put, put-files; one line per format
//!                                         for put-uri-list-and-files)
//! crcbl-e2e-win32-clip: file <path>      (get-files, one line per file)
//! crcbl-e2e-win32-clip: effect <dword>   (get-files, the Preferred DropEffect)
//! crcbl-e2e-win32-clip: effect absent    (get-files, when there is none)
//! crcbl-e2e-win32-clip: holding <ms>     (hold, once the clipboard is open)
//! crcbl-e2e-win32-clip: released         (hold, once it is closed again)
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

/// The clipboard surface of `user32`, `kernel32` and `shell32`, hand-written like
/// every other declaration in this crate.
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
    /// `CF_HDROP` — a `DROPFILES` header followed by a file list.
    pub const CF_HDROP: u32 = 15;
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

    /// `DragQueryFileW`'s `index` that asks for the file count.
    pub const DRAG_QUERY_COUNT: u32 = 0xFFFF_FFFF;

    #[link(name = "shell32")]
    unsafe extern "system" {
        /// With `index` [`DRAG_QUERY_COUNT`], the number of files an `HDROP`
        /// names; otherwise that file's length in `WCHAR`s without its NUL,
        /// and its path copied into `buffer` when that is not null.
        pub fn DragQueryFileW(drop: Handle, index: u32, buffer: *mut u16, capacity: u32) -> u32;
    }
}

#[cfg(target_os = "windows")]
fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let verb = args.next();
    if verb.as_deref() == Some("get-files") {
        return report(get_files());
    }
    let (Some(verb), Some(format)) = (verb, args.next()) else {
        eprintln!(
            "crcbl-e2e-win32-clip: usage: crcbl-e2e-win32-clip <get|put> <format> [text…] | hold \
             <ms> | put-files <path> [path…] | put-uri-list-and-files <uri-list> <path> \
             [path…] | get-files"
        );
        return ExitCode::from(2);
    };
    if verb == "put-files" {
        // Every word is a path here, the first one included.
        let paths: Vec<String> = core::iter::once(format).chain(args).collect();
        return report(publish(&[(
            "CF_HDROP",
            win32::CF_HDROP,
            drop_files(&paths),
        )]));
    }
    if verb == "put-uri-list-and-files" {
        // The first word is the whole uri-list here; every later one is a path.
        let paths: Vec<String> = args.collect();
        return report(format_id(URI_LIST).and_then(|uri_list| {
            publish(&[
                (URI_LIST, uri_list, encode(uri_list, &format)),
                ("CF_HDROP", win32::CF_HDROP, drop_files(&paths)),
            ])
        }));
    }
    if verb == "hold" {
        // The second word is a duration here, not a format.
        return report(
            format
                .parse::<u64>()
                .map_err(|_| format!("{format:?} is not a whole number of milliseconds"))
                .and_then(|ms| hold(std::time::Duration::from_millis(ms))),
        );
    }
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
            eprintln!("crcbl-e2e-win32-clip: {other:?} is not get, put or hold");
            return ExitCode::from(2);
        }
    };
    report(outcome)
}

/// The registered format a `text/uri-list` travels under.
#[cfg(target_os = "windows")]
const URI_LIST: &str = "text/uri-list";

/// The registered format Explorer reads to choose between copying and moving
/// the files a `CF_HDROP` names.
#[cfg(target_os = "windows")]
const PREFERRED_DROP_EFFECT: &str = "Preferred DropEffect";

/// The exit status a verb's outcome ends the process with, the problem said on
/// stderr.
#[cfg(target_os = "windows")]
fn report(outcome: Result<(), String>) -> ExitCode {
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
    publish(&[(name, format, encode(format, text))])
}

/// Publishes each `(name, format, payload)`, verbatim, as the only formats on
/// the clipboard, in one write.
///
/// As with [`put`], the blocks outlive this process on success.
#[cfg(target_os = "windows")]
fn publish(formats: &[(&str, u32, Vec<u8>)]) -> Result<(), String> {
    let clipboard = Clipboard::open()?;
    // SAFETY: the clipboard is open on this thread. `EmptyClipboard` is required
    // before a write and is what makes this process the owner.
    unsafe { win32::EmptyClipboard() };
    for (name, format, payload) in formats {
        let block = moveable_block(payload)?;
        // SAFETY: as above. On success the window station takes ownership of
        // `block`, which is why it is not freed below.
        let published = unsafe { win32::SetClipboardData(*format, block) };
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
    }
    drop(clipboard);
    Ok(())
}

/// A fresh `GMEM_MOVEABLE` block holding `payload`, owned by the caller until
/// `SetClipboardData` takes it.
#[cfg(target_os = "windows")]
fn moveable_block(payload: &[u8]) -> Result<win32::Handle, String> {
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
    Ok(block)
}

/// Reads the `CF_HDROP` on the clipboard through `DragQueryFileW` and prints
/// each path, then the `Preferred DropEffect` beside it.
///
/// `DragQueryFileW` rather than parsing the `DROPFILES` block by hand, because
/// that is what a pasting application calls: a header at the wrong offset, or
/// an `fWide` of `FALSE`, is what it would misread, and this has to misread it
/// the same way.
#[cfg(target_os = "windows")]
fn get_files() -> Result<(), String> {
    // SAFETY: a format id by value; the call only reads window-station state and
    // needs no open clipboard.
    if unsafe { win32::IsClipboardFormatAvailable(win32::CF_HDROP) } == 0 {
        println!("crcbl-e2e-win32-clip: absent");
        return Ok(());
    }
    let effect_format = format_id(PREFERRED_DROP_EFFECT)?;

    let clipboard = Clipboard::open()?;
    // SAFETY: the clipboard is open on this thread for as long as `clipboard`
    // lives. The block belongs to the window station: it is only queried, never
    // freed or finished.
    let hdrop = unsafe { win32::GetClipboardData(win32::CF_HDROP) };
    if hdrop.is_null() {
        return Err(
            "CF_HDROP was available and GetClipboardData answered null; something else wrote to \
             the clipboard mid-read"
                .to_owned(),
        );
    }
    // SAFETY: `hdrop` is a live `HDROP` for as long as the clipboard is open,
    // and a null buffer asks for the count only.
    let count =
        unsafe { win32::DragQueryFileW(hdrop, win32::DRAG_QUERY_COUNT, core::ptr::null_mut(), 0) };
    let mut paths = Vec::new();
    for index in 0..count {
        // SAFETY: as above; a null buffer asks for the length without the NUL.
        let length = unsafe { win32::DragQueryFileW(hdrop, index, core::ptr::null_mut(), 0) };
        let mut buffer = vec![0u16; length as usize + 1];
        // SAFETY: `buffer` holds `length + 1` units, which is the capacity
        // passed, so the path and its NUL fit.
        let copied =
            unsafe { win32::DragQueryFileW(hdrop, index, buffer.as_mut_ptr(), length + 1) };
        paths.push(String::from_utf16_lossy(&buffer[..copied as usize]));
    }
    // SAFETY: as for `CF_HDROP` above.
    let effect_block = unsafe { win32::GetClipboardData(effect_format) };
    let effect = if effect_block.is_null() {
        None
    } else {
        // SAFETY: a live block the window station owns, locked for the copy and
        // unlocked straight after. Four bytes are read only when the block
        // reports at least four.
        unsafe {
            let locked = win32::GlobalLock(effect_block);
            let value = (!locked.is_null() && win32::GlobalSize(effect_block) >= 4)
                .then(|| locked.cast::<u32>().read_unaligned());
            if !locked.is_null() {
                win32::GlobalUnlock(effect_block);
            }
            value
        }
    };
    drop(clipboard);

    for path in paths {
        println!("crcbl-e2e-win32-clip: file {path}");
    }
    match effect {
        Some(effect) => println!("crcbl-e2e-win32-clip: effect {effect}"),
        None => println!("crcbl-e2e-win32-clip: effect absent"),
    }
    Ok(())
}

/// Opens the clipboard, says so, keeps it open for `span`, and gives it back.
///
/// The contention a clipboard manager or a slow application causes, on demand:
/// while this holds it, every other process's `OpenClipboard` is refused. The
/// `holding` line is printed only once the open has succeeded, and flushed, so
/// a caller that waits for it knows the clipboard is taken from that moment
/// until `released`.
#[cfg(target_os = "windows")]
fn hold(span: std::time::Duration) -> Result<(), String> {
    use std::io::Write;

    let clipboard = Clipboard::open()?;
    let mut out = std::io::stdout().lock();
    writeln!(out, "crcbl-e2e-win32-clip: holding {}", span.as_millis())
        .and_then(|()| out.flush())
        .map_err(|error| format!("could not say the clipboard is held: {error}"))?;
    std::thread::sleep(span);
    drop(clipboard);
    writeln!(out, "crcbl-e2e-win32-clip: released")
        .and_then(|()| out.flush())
        .map_err(|error| format!("could not say the clipboard is released: {error}"))
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

/// The `CF_HDROP` block naming `paths`, as `shlobj_core.h` lays it out.
///
/// A `DROPFILES` header — `pFiles`, the list's offset from the start of the
/// block; `pt`, a `POINT`; `fNC`; and `fWide` — is 20 bytes of 4-byte fields
/// under `pshpack1.h`. It is followed by each path in UTF-16 with its NUL, and
/// one more NUL that ends the list. `fWide` is `TRUE` because the list is wide.
///
/// Built as bytes rather than through a `#[repr(C)]` struct: the header is only
/// ever written here, so spelling out its five fields is the whole of the
/// layout, with nothing for a padding rule to disagree about.
#[cfg(target_os = "windows")]
fn drop_files(paths: &[String]) -> Vec<u8> {
    const HEADER: u32 = 20;
    let mut block = Vec::new();
    for field in [HEADER, 0, 0, 0, 1] {
        // pFiles, pt.x, pt.y, fNC, fWide.
        block.extend_from_slice(&field.to_ne_bytes());
    }
    let list = paths
        .iter()
        .flat_map(|path| path.encode_utf16().chain(core::iter::once(0)))
        .chain(core::iter::once(0));
    for unit in list {
        block.extend_from_slice(&unit.to_ne_bytes());
    }
    block
}

/// The reverse, stopping at the terminator and tolerating padding after it.
#[cfg(target_os = "windows")]
fn decode(format: u32, bytes: &[u8]) -> String {
    if format == win32::CF_UNICODETEXT {
        let mut units = Vec::with_capacity(bytes.len() / 2);
        for pair in bytes.as_chunks::<2>().0 {
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
/// `docs/notes/process.md` asks for instead of a sleep.
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
