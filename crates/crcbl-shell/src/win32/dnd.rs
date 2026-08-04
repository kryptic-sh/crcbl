//! File drops: `DragAcceptFiles`, `WM_DROPFILES`, and turning an `HDROP` into
//! paths.
//!
//! Deliberately **not** in [`clipboard`](super::clipboard), although the two
//! landed in one slice. On Wayland and X11 the clipboard and drag-and-drop are
//! "one implementation, two triggers" — the same offer object reached two ways —
//! and `docs/plan/15-windowing.md` says so. On Win32 they are two unrelated
//! mechanisms that happen to move the same kind of data: the clipboard is a
//! window-station object read with `GetClipboardData`, and a drop is a shell
//! object delivered as a window message. Putting them in one module would
//! suggest a relationship that is not there.
//!
//! # Decision: `WM_DROPFILES`, not `IDropTarget`
//!
//! Windows has two drop APIs and the small one does the job the seam asks for.
//!
//! [`ShellEvent::DroppedFile`](crate::ShellEvent::DroppedFile) is *files, in,
//! with a position* — the same shape the Wayland backend emits, and
//! `docs/plan/15-windowing.md` scopes drag-and-drop to "file paths in
//! (viewer/editor import)". `DragAcceptFiles` plus `WM_DROPFILES` delivers
//! exactly that: an `HDROP` carrying the paths and the client-space point, in
//! one message, with the accept gate enforced by the system.
//!
//! `RegisterDragDrop` and `IDropTarget` would add what the seam has no way to
//! express and no consumer for: `DragEnter`/`DragOver` feedback while the drag
//! is still in the air (a drop cursor, a hover highlight), non-file formats, and
//! a choice of copy-versus-move. The cost is not the interface, it is **COM**:
//! `OleInitialize` on the pumping thread, a hand-written vtable with `IUnknown`
//! reference counting, an apartment this crate does not own and cannot uninit
//! without knowing what else the host process put in it. That is a slice, and it
//! buys drag *feedback* rather than drops.
//!
//! What is genuinely given up is named rather than glossed: while a file is
//! being dragged over the window, nothing highlights and the cursor is the
//! system's default "no target" one until the user lets go. The drop itself
//! works. `docs/backlog.md` carries it, for the editor's asset browser at P12.
//!
//! # The gate is the system's, and this backend checks it anyway
//!
//! [`WindowDesc::accept_drops`](crate::WindowDesc::accept_drops) is off by
//! default because "accepting a drop means advertising it to the window system",
//! and here that advertisement is literal: `DragAcceptFiles` sets
//! `WS_EX_ACCEPTFILES` and **no `WM_DROPFILES` is ever sent to a window without
//! it**. That is a stronger gate than the Wayland backend's, which has to record
//! the refusal itself and answer `accept(null)`.
//!
//! The shell still checks the flag when it translates the message, for the case
//! the system cannot cover: the extended style is a *window* property that
//! anything in the process can set, and a window this backend was told not to
//! accept drops for must not start reporting them because something else — a
//! host application, an injected library — turned the bit on.
//!
//! # The paths are read in the window procedure, and they have to be
//!
//! An `HDROP` is live until `DragFinish`, which the receiver owes: skipping it
//! leaks shell memory, and keeping the handle to dereference on the next
//! [`pump`](crate::Shell::pump) would put a raw system handle in a queue that
//! [`events`](super::events) is explicit holds no such thing. So the procedure
//! reads the paths and finishes the handle before it returns.
//!
//! That produces the one thing a [`RawEvent`](super::events::RawEvent) cannot
//! carry — a `Vec<PathBuf>` in a `Copy` enum — so the payload goes into a queue
//! of its own on [`Shared`](super::proc::Shared) and the raw event is a
//! **marker** that keeps the drop in its place in the event stream. A drop that
//! followed the pointer motion which positioned it must still follow it.

use std::path::PathBuf;

/// One path as `DragQueryFileW` wrote it, or `None` if there is nothing there.
///
/// Trimmed at the first NUL rather than taken whole: the buffer is sized from
/// the length the call reported and then filled by a second call, and a
/// defensive read of the whole buffer would otherwise append the terminator —
/// and anything after it — to the path. An empty path is not a file.
///
/// On Windows a path is UTF-16 and `OsString::from_wide` is lossless, so a name
/// this process cannot display still opens. Everywhere else — which is only the
/// host this was developed on, under `cfg(test)` — the same decoding is done
/// lossily, so the pure part is exercised without a window station.
#[must_use]
pub fn path_from_wide(units: &[u16]) -> Option<PathBuf> {
    let end = units
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(units.len());
    let units = &units[..end];
    if units.is_empty() {
        return None;
    }
    Some(decode(units))
}

/// See [`path_from_wide`].
#[cfg(target_os = "windows")]
fn decode(units: &[u16]) -> PathBuf {
    use std::os::windows::ffi::OsStringExt;
    PathBuf::from(std::ffi::OsString::from_wide(units))
}

/// See [`path_from_wide`].
#[cfg(not(target_os = "windows"))]
fn decode(units: &[u16]) -> PathBuf {
    PathBuf::from(String::from_utf16_lossy(units))
}

#[cfg(target_os = "windows")]
pub(super) use system::{accept_files, take_drop};

/// The half that calls `shell32`.
#[cfg(target_os = "windows")]
mod system {
    use std::path::PathBuf;

    use super::super::ffi::{self, Handle, Point, value};
    use super::path_from_wide;

    /// Turns a window's file drops on or off.
    ///
    /// Sets `WS_EX_ACCEPTFILES`, which is why anything that rewrites the
    /// extended style word has to carry the bit across — see
    /// [`style::EX_ACCEPT_FILES`](super::super::ffi::style::EX_ACCEPT_FILES).
    pub(in super::super) fn accept_files(hwnd: Handle, accept: bool) {
        // SAFETY: a live window of this shell and an integer by value. The call
        // toggles one bit of the window's extended style and cannot fail in a
        // way that leaves state half-changed.
        unsafe { ffi::DragAcceptFiles(hwnd, i32::from(accept)) };
    }

    /// Reads an `HDROP` and releases it.
    ///
    /// Answers the paths and the drop point in **client** coordinates, which is
    /// what [`DroppedFile::position`](crate::ShellEvent::DroppedFile) means.
    ///
    /// # Safety
    ///
    /// `hdrop` must be the `wParam` of a `WM_DROPFILES` currently being
    /// processed. It is finished here and must not be used afterwards.
    pub(in super::super) unsafe fn take_drop(hdrop: Handle) -> (Vec<PathBuf>, Point) {
        // SAFETY: the caller guarantees the handle. `DRAG_QUERY_COUNT` with a
        // null buffer asks for the file count and writes nothing.
        let count = unsafe { ffi::DragQueryFileW(hdrop, value::DRAG_QUERY_COUNT, ptr_null(), 0) };
        let mut paths = Vec::with_capacity(count as usize);
        for index in 0..count {
            // SAFETY: as above; an index with a null buffer answers the length
            // in characters, excluding the terminator.
            let length = unsafe { ffi::DragQueryFileW(hdrop, index, ptr_null(), 0) };
            if length == 0 {
                continue;
            }
            // The buffer is the reported length plus the terminator the call
            // always writes. Sized from the answer rather than from `MAX_PATH`,
            // which stopped being the limit once long paths were opted into.
            let mut buffer = vec![0u16; length as usize + 1];
            // SAFETY: as above; `buffer` is a live, initialised array of
            // exactly the number of code units passed as `size`, which is what
            // the call is documented to write at most.
            let written = unsafe {
                ffi::DragQueryFileW(hdrop, index, buffer.as_mut_ptr(), buffer.len() as u32)
            };
            if let Some(path) = path_from_wide(&buffer[..(written as usize).min(buffer.len())]) {
                paths.push(path);
            }
        }

        let mut point = Point::default();
        // SAFETY: the caller guarantees the handle, and `point` is a live,
        // initialised `POINT` the call writes into. A drop outside the client
        // area answers `FALSE` and leaves the origin, which is the honest
        // answer for a position nobody can use.
        unsafe { ffi::DragQueryPoint(hdrop, &raw mut point) };

        // SAFETY: the handle has been read and is released exactly once, here.
        // Nothing above retained it.
        unsafe { ffi::DragFinish(hdrop) };
        (paths, point)
    }

    /// A null `LPWSTR`, for the two `DragQueryFileW` queries that write nothing.
    const fn ptr_null() -> *mut u16 {
        core::ptr::null_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A NUL-terminated wide buffer, as `DragQueryFileW` fills one in.
    fn wide(text: &str) -> Vec<u16> {
        let mut units: Vec<u16> = text.encode_utf16().collect();
        units.push(0);
        units
    }

    #[test]
    fn a_queried_path_stops_at_the_terminator_and_keeps_everything_before_it() {
        // The buffer is sized from a length the call reported and filled by a
        // second call, so it always carries a terminator and may carry more
        // than the path. Reading it whole appends a NUL to every dropped path,
        // which then fails to open with a name that *looks* right in a log.
        assert_eq!(
            path_from_wide(&wide(r"C:\Users\dev\My Scene.ron")),
            Some(PathBuf::from(r"C:\Users\dev\My Scene.ron"))
        );

        // Slack after the terminator, which a buffer sized generously has.
        let mut roomy = wide(r"C:\a.png");
        roomy.extend_from_slice(&[0; 12]);
        assert_eq!(path_from_wide(&roomy), Some(PathBuf::from(r"C:\a.png")));

        // A buffer with no terminator at all is still a path.
        let exact: Vec<u16> = r"C:\b.png".encode_utf16().collect();
        assert_eq!(path_from_wide(&exact), Some(PathBuf::from(r"C:\b.png")));
    }

    #[test]
    fn an_empty_entry_is_not_a_path() {
        // `DragQueryFileW` answering a zero length, and a buffer that is
        // nothing but the terminator. Neither names a file, and pushing
        // `PathBuf::from("")` would emit a `DroppedFile` for it.
        assert_eq!(path_from_wide(&[]), None);
        assert_eq!(path_from_wide(&[0]), None);
        assert_eq!(path_from_wide(&[0, 0, 0]), None);
    }

    #[test]
    fn a_unc_path_and_a_non_ascii_name_both_survive() {
        // Two shapes a drop really produces: a network share, and a name in a
        // script this process may not be able to display. Neither is special
        // to the decoder, and that is the claim.
        assert_eq!(
            path_from_wide(&wide(r"\\server\share\level.ron")),
            Some(PathBuf::from(r"\\server\share\level.ron"))
        );
        assert_eq!(
            path_from_wide(&wide(r"C:\プロジェクト\🎮.png")),
            Some(PathBuf::from(r"C:\プロジェクト\🎮.png")),
            "a surrogate pair is one character on both sides of the conversion"
        );
    }
}
