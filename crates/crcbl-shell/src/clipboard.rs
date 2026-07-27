//! Clipboard and drag-and-drop payloads.
//!
//! # Decision: the trait surface shipped a slice before the backends
//!
//! P0.4 implemented no platform clipboard — there was no platform code in that
//! slice at all — and P0.5c implemented the Wayland one against this shape
//! unchanged, which is the outcome the trade below was made for. The *shape*
//! was here first, and that was deliberate:
//! adding a method to a trait that three backends already implement means
//! touching three backends, whereas leaving an unimplemented method on the
//! trait costs nothing but the `Unsupported` arm each of them writes once.
//! `docs/plan/15-windowing.md` also settles the design questions that would
//! otherwise make this speculative — the mime set is specified, and clipboard
//! and drag-and-drop are stated to share one implementation on Wayland and X11
//! ("one implementation, two triggers"), so the payload types below serve both.
//!
//! [`HeadlessShell`](crate::HeadlessShell) implements the surface fully, in
//! process, which means the shape is exercised by tests rather than merely
//! asserted to be right.
//!
//! # Decision: reads are asynchronous, like `crcbl-hal`'s readback
//!
//! There is no `fn clipboard_text(&self) -> Option<String>`. A read is
//! [`clipboard_request`](crate::Shell::clipboard_request), which returns a
//! [`ClipboardRequestId`], and the answer arrives later as
//! [`ShellEvent::ClipboardData`](crate::ShellEvent::ClipboardData).
//!
//! This is the same argument `crcbl-hal` makes for polled readback, and it has
//! the same force. A synchronous getter is unimplementable on:
//!
//! * **X11**, where reading a selection is a round trip to the *owning client*
//!   — `ConvertSelection`, wait for `SelectionNotify`, and for anything large
//!   an entire `INCR` chunked transfer driven by property-change events. A
//!   blocking implementation deadlocks the moment the owner is the same
//!   process, which in an editor with two windows it is.
//! * **Wayland**, where a `data_offer` hands over a file descriptor to read.
//! * **The browser**, where `navigator.clipboard.read()` returns a Promise and
//!   is permission-gated behind a user gesture.
//!
//! Three of five backends cannot implement the synchronous shape, so it is not
//! the shape. Writes, by contrast, are synchronous: every platform's write is
//! "advertise that we own the selection", which completes immediately — the
//! *transfer* happens later, on demand, and the shell owns the bytes until it
//! does.
//!
//! # Decision: a read that cannot be answered yet is *held*, not answered
//!
//! P0.5c's revision, and the reason for it is a finding rather than a
//! preference. On Wayland a client is told what is on the clipboard by
//! `wl_data_device.selection`, which arrives **asynchronously** and **only
//! while one of its windows has keyboard focus**. A read issued a millisecond
//! too early — during the focus change a Ctrl+V *itself* may have caused — has
//! nothing to answer from.
//!
//! Answering "empty" there is a lie a consumer cannot detect, and the end-to-end
//! suite proved it: it needed a retry loop that no editor's paste command would
//! ever contain. So the shape is that a backend **holds** the request until it
//! is answerable and only then emits
//! [`ShellEvent::ClipboardData`](crate::ShellEvent::ClipboardData). There is
//! deliberately no "try again later" outcome; see [`ClipboardContent`].
//!
//! This costs X11 nothing, which is the test every seam change here has to
//! pass: an X11 backend can answer immediately (`XGetSelectionOwner` says
//! whether anyone owns the selection, with no focus involved), so "hold until
//! answerable" collapses to "answer". A backend where the wait is real bounds
//! it and reports [`ClipboardContent::Unavailable`] rather than waiting
//! forever, and [`Shell::clipboard_readable`](crate::Shell::clipboard_readable)
//! lets a UI ask in advance instead of discovering it by waiting.
//!
//! # Two mime types, always offered together
//!
//! `15-windowing.md` specifies `text/plain` plus the custom
//! `application/x-crcbl+ron` so that engine↔engine copies are lossless while
//! outside applications still receive readable text. Offering both is the
//! caller's job, and [`ClipboardOffer`] is a slice for exactly that reason.
//!
//! # `text/uri-list` is parsed here, not in a backend
//!
//! A file drop and a "copy file" paste both arrive as a `text/uri-list` blob,
//! on Wayland (`wl_data_offer`), on X11 (`XdndSelection`) and in a browser
//! (`DataTransfer`). The format is RFC 2483's, the same on all three, and
//! getting it wrong — percent-encoding, `file://` authorities, CRLF, comment
//! lines — is wrong in the same way on all three. So [`parse_uri_list`] lives
//! next to the mime types rather than being written once per backend, which is
//! how the second and third copies drift.

use core::fmt;
use std::path::PathBuf;

/// The format of a clipboard or drag-and-drop payload.
///
/// A small enum with an escape hatch rather than a bare string, so the two
/// formats the engine cares about cannot be misspelled, and so a `match` on the
/// interesting cases reads as one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MimeType {
    /// `text/plain;charset=utf-8` — what every other application understands.
    ///
    /// Backends must accept the `text/plain` spelling without the charset
    /// parameter as equivalent when *receiving*, and offer the explicit
    /// charset when sending: X11 clients in particular offer both `STRING` and
    /// `UTF8_STRING` and mean this.
    TextUtf8,

    /// `application/x-crcbl+ron` — the engine's own lossless format.
    ///
    /// Copying a scene node between two editor windows goes through this;
    /// pasting into a text editor goes through [`TextUtf8`](Self::TextUtf8),
    /// which is why both are offered at once.
    CrcblRon,

    /// `text/uri-list` — file paths, the format both a file-manager drop and a
    /// "copy file" paste arrive in.
    UriList,

    /// Anything else, by literal mime string.
    ///
    /// `&'static str` rather than `String` keeps [`MimeType`] `Copy` and keeps
    /// the whole payload type free of allocation on the send path. A backend
    /// receiving a mime it has no variant for reports it here.
    Other(&'static str),
}

impl MimeType {
    /// The wire form of this mime type.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TextUtf8 => "text/plain;charset=utf-8",
            Self::CrcblRon => "application/x-crcbl+ron",
            Self::UriList => "text/uri-list",
            Self::Other(mime) => mime,
        }
    }

    /// Recognizes a mime string the window system offered.
    ///
    /// Accepts the `text/plain` spelling without a charset as
    /// [`TextUtf8`](Self::TextUtf8): X11 and the browser both produce it, and
    /// treating it as an unknown format would mean refusing the single most
    /// common paste there is.
    #[must_use]
    pub fn recognize(mime: &str) -> Option<Self> {
        match mime {
            "text/plain;charset=utf-8" | "text/plain" | "UTF8_STRING" | "STRING" => {
                Some(Self::TextUtf8)
            }
            "application/x-crcbl+ron" => Some(Self::CrcblRon),
            "text/uri-list" => Some(Self::UriList),
            _ => None,
        }
    }
}

impl fmt::Display for MimeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A mime type as some *other* application spelled it.
///
/// # Why this is a second type
///
/// [`MimeType`] is `Copy`, which means its escape hatch is
/// [`Other(&'static str)`](MimeType::Other) — fine for something this engine
/// names in its own source, and structurally unable to hold a string that
/// arrived at runtime from another process. Every incoming mime is exactly
/// that: an X11 `TARGETS` reply, a Wayland `data_offer.offer`, or a browser
/// `DataTransfer.types` entry, all of them arbitrary and none of them
/// `'static`.
///
/// So the outgoing side stays `Copy` and the incoming side owns its string. The
/// asymmetry is the point: a clipboard write names a format the engine chose,
/// and a clipboard read reports a format someone else chose.
///
/// # Fidelity
///
/// The literal spelling is preserved rather than canonicalized.
/// [`recognized`](Self::recognized) maps it onto a [`MimeType`] where one fits,
/// but `ReceivedMime::new("text/plain")` keeps `"text/plain"` — because a
/// backend answering an X11 `SelectionRequest` has to echo back the *target
/// atom that was asked for*, and rewriting it to
/// `text/plain;charset=utf-8` would produce a reply the peer does not
/// recognize.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ReceivedMime(std::borrow::Cow<'static, str>);

impl ReceivedMime {
    /// Wraps a mime string that arrived from another application.
    ///
    /// Borrows rather than allocating when the string is byte-identical to one
    /// of the engine's own — which covers the overwhelmingly common case of a
    /// crcbl-to-crcbl copy — and allocates otherwise.
    #[must_use]
    pub fn new(mime: &str) -> Self {
        for known in [MimeType::TextUtf8, MimeType::CrcblRon, MimeType::UriList] {
            if known.as_str() == mime {
                return Self(std::borrow::Cow::Borrowed(known.as_str()));
            }
        }
        Self(std::borrow::Cow::Owned(mime.to_string()))
    }

    /// The mime string exactly as it was received.
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The [`MimeType`] this corresponds to, if the engine knows the format.
    ///
    /// `None` means "some other application's format" — loggable, forwardable,
    /// and not something the engine can interpret.
    #[must_use]
    pub fn recognized(&self) -> Option<MimeType> {
        MimeType::recognize(&self.0)
    }

    /// Whether this is the format `mime` names, allowing for the alternative
    /// spellings [`MimeType::recognize`] accepts.
    ///
    /// This is the comparison a paste handler wants: asking for
    /// [`MimeType::TextUtf8`] and receiving `"text/plain"` is a match, and
    /// comparing the strings directly would say it is not.
    #[must_use]
    pub fn matches(&self, mime: MimeType) -> bool {
        self.recognized() == Some(mime) || self.0 == mime.as_str()
    }
}

impl From<MimeType> for ReceivedMime {
    fn from(mime: MimeType) -> Self {
        Self(std::borrow::Cow::Borrowed(mime.as_str()))
    }
}

impl fmt::Display for ReceivedMime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One format of a payload being offered to the window system.
///
/// A clipboard write offers a slice of these — the same bytes in several
/// formats, or genuinely different renderings of one thing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClipboardOffer<'a> {
    /// What format `bytes` is in.
    pub mime: MimeType,
    /// The payload. Not required to be valid UTF-8 even for
    /// [`MimeType::TextUtf8`] — validation belongs to whoever built it, and a
    /// shell that re-validated every write would be doing it twice.
    pub bytes: &'a [u8],
}

impl<'a> ClipboardOffer<'a> {
    /// A `text/plain` offer.
    #[inline]
    #[must_use]
    pub const fn text(text: &'a str) -> Self {
        Self {
            mime: MimeType::TextUtf8,
            bytes: text.as_bytes(),
        }
    }

    /// An `application/x-crcbl+ron` offer.
    #[inline]
    #[must_use]
    pub const fn ron(ron: &'a str) -> Self {
        Self {
            mime: MimeType::CrcblRon,
            bytes: ron.as_bytes(),
        }
    }
}

/// What a clipboard read produced.
///
/// # Decision: three outcomes, because two of them were being conflated
///
/// This started life as `Option<Vec<u8>>`, on the argument that "the clipboard
/// was empty", "it held no compatible format" and "the transfer failed" all
/// mean *there is nothing to paste* and a caller does the same thing for each.
/// P0.5c found that wrong against a real compositor, in two ways that a caller
/// genuinely must tell apart:
///
/// * **A successful transfer of zero bytes is not a failure.** A peer may
///   publish an empty selection, and [`Bytes(vec![])`](Self::Bytes) says so.
///   Reporting `None` there tells an editor its paste failed when it did not.
/// * **"Could not be read" is not "there is nothing there".** A transfer whose
///   peer went away mid-write, or which never became readable at all, is a
///   failure a UI may want to surface ("the clipboard could not be read") and
///   which an automation script must not silently treat as an empty clipboard.
///
/// [`Empty`](Self::Empty) still merges "the clipboard holds nothing" with "it
/// holds nothing in *that* format", because those two really are one case: the
/// caller named the format, and the answer is that it is not on offer. A caller
/// that wants a different format asks for it, which is one more request either
/// way.
///
/// What is deliberately **not** here is "not yet". A read that cannot be
/// answered yet is *held* until it can be — see
/// [`clipboard_request`](crate::Shell::clipboard_request) — because a variant
/// meaning "ask again later" is a retry loop in every consumer, and the
/// consumer has no idea how long to wait.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClipboardContent {
    /// The payload, in the format [`ClipboardData::mime`](crate::ShellEvent::ClipboardData)
    /// names. May be empty: that is a successful transfer of nothing.
    Bytes(Vec<u8>),

    /// The clipboard holds nothing, or nothing in the requested format.
    Empty,

    /// The read could not be completed.
    ///
    /// The window system refused it, the peer holding the data went away or
    /// never delivered, or the window never became able to read the clipboard
    /// within the backend's deadline. Distinct from [`Empty`](Self::Empty):
    /// there may well be something on the clipboard.
    Unavailable,
}

impl ClipboardContent {
    /// The payload, if the read succeeded.
    ///
    /// `None` for both [`Empty`](Self::Empty) and
    /// [`Unavailable`](Self::Unavailable) — the "just give me the bytes"
    /// accessor, for a caller that treats every non-answer the same.
    #[inline]
    #[must_use]
    pub fn bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Bytes(bytes) => Some(bytes),
            Self::Empty | Self::Unavailable => None,
        }
    }

    /// The payload, consumed. See [`bytes`](Self::bytes).
    #[inline]
    #[must_use]
    pub fn into_bytes(self) -> Option<Vec<u8>> {
        match self {
            Self::Bytes(bytes) => Some(bytes),
            Self::Empty | Self::Unavailable => None,
        }
    }

    /// The payload as text, when it is valid UTF-8.
    ///
    /// The overwhelmingly common paste, and the one place it is worth saving
    /// every caller the same three lines. Invalid UTF-8 answers `None` rather
    /// than being lossily replaced: a paste that silently corrupts what was
    /// copied is worse than one that refuses.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        core::str::from_utf8(self.bytes()?).ok()
    }

    /// A short, stable name for logs and test assertions.
    #[inline]
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Bytes(_) => "Bytes",
            Self::Empty => "Empty",
            Self::Unavailable => "Unavailable",
        }
    }
}

/// Identifies an outstanding [`clipboard_request`](crate::Shell::clipboard_request).
///
/// Matched against
/// [`ShellEvent::ClipboardData::request`](crate::ShellEvent::ClipboardData) when
/// the answer arrives. Ids are unique within a shell for the session; a caller
/// with several reads in flight — an editor paste racing an asset-browser drop
/// — tells them apart by this and nothing else.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClipboardRequestId(pub u32);

impl fmt::Display for ClipboardRequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "clipboard request {}", self.0)
    }
}

/// The local paths named by a [`MimeType::UriList`] payload.
///
/// A dropped file does not arrive as a path. It arrives as RFC 2483's
/// `text/uri-list`: CRLF-separated URIs, percent-encoded, with `#` comment
/// lines permitted. This turns that into the paths
/// [`ShellEvent::DroppedFile`](crate::ShellEvent::DroppedFile) carries.
///
/// # What is dropped, and why
///
/// * **Comment lines and blank lines.** The format defines both.
/// * **Non-`file:` URIs.** A file manager can put `https://…`, `trash:///…` or
///   `smb://host/share` in a drop, and none of them is a path — turning
///   `https://example.com/a` into the relative path `example.com/a` would be a
///   plausible-looking lie that only fails once something tries to open it.
///   They are skipped, so a drop of three URLs and one file produces one
///   [`DroppedFile`](crate::ShellEvent::DroppedFile).
/// * **`file://` with a remote authority.** `file://host/path` names a file on
///   *that* host. `file:///path` (empty authority) and `file://localhost/path`
///   are this machine and are kept; anything else is not reachable through a
///   `PathBuf`.
///
/// Percent-decoding happens **after** the scheme and authority are stripped, so
/// a `%2F` inside a filename decodes to a `/` in the name rather than to a path
/// separator that changes the URI's structure — the ordering RFC 3986 requires
/// and the one a naive "decode the whole line first" gets wrong.
///
/// Paths are bytes on Unix, so a name that is not valid UTF-8 survives intact.
///
/// ```
/// use crcbl_shell::parse_uri_list;
/// use std::path::PathBuf;
///
/// let dropped = b"# comment\r\nfile:///tmp/my%20scene.ron\r\nhttps://example.com/x\r\n";
/// assert_eq!(
///     parse_uri_list(dropped),
///     vec![PathBuf::from("/tmp/my scene.ron")],
///     "the URL is not a path and the space is decoded",
/// );
/// ```
#[must_use]
pub fn parse_uri_list(bytes: &[u8]) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    // Split on LF and trim a trailing CR, rather than splitting on CRLF: the
    // format says CRLF, real senders send both, and a parser that insisted on
    // CRLF would silently return nothing for half of them.
    for line in bytes.split(|byte| *byte == b'\n') {
        let line = trim_ascii(line);
        // `#` is a comment *line*; a `#` inside a URI is a fragment and is not
        // this case.
        if line.is_empty() || line[0] == b'#' {
            continue;
        }
        if let Some(path) = file_uri_to_path(line) {
            paths.push(path);
        }
    }
    paths
}

/// One `file:` URI as a path, or `None` for anything that is not a local file.
fn file_uri_to_path(uri: &[u8]) -> Option<PathBuf> {
    const SCHEME: &[u8] = b"file:";
    let (prefix, rest) = uri.split_at_checked(SCHEME.len())?;
    if !prefix.eq_ignore_ascii_case(SCHEME) {
        return None;
    }

    let path = if let Some(authority) = rest.strip_prefix(b"//") {
        // `file://<authority>/<path>`. The authority ends at the first `/`,
        // which is also the first byte of the path.
        let split = authority.iter().position(|byte| *byte == b'/')?;
        let (host, path) = authority.split_at(split);
        if !(host.is_empty() || host.eq_ignore_ascii_case(b"localhost")) {
            return None;
        }
        path
    } else if rest.starts_with(b"/") {
        // `file:/path` — RFC 8089 permits the authority to be omitted entirely.
        rest
    } else {
        // `file:relative` is not a thing this can resolve against anything.
        return None;
    };

    let decoded = percent_decode(path);
    if decoded.is_empty() {
        return None;
    }
    bytes_to_path(decoded)
}

/// RFC 3986 percent-decoding, byte-wise.
///
/// A `%` that is not followed by two hex digits is a literal `%`: senders do
/// emit them, and refusing the whole line over one would lose a real file.
fn percent_decode(bytes: &[u8]) -> Vec<u8> {
    const fn hex(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    }
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && let (Some(high), Some(low)) = (
                bytes.get(index + 1).copied().and_then(hex),
                bytes.get(index + 2).copied().and_then(hex),
            )
        {
            out.push((high << 4) | low);
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    out
}

/// Decoded URI bytes as a path.
///
/// Unix paths are arbitrary bytes, and a file whose name is not valid UTF-8 is
/// still a file that can be dropped on a window, so the bytes are taken as-is.
#[cfg(unix)]
fn bytes_to_path(bytes: Vec<u8>) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    Some(PathBuf::from(std::ffi::OsString::from_vec(bytes)))
}

/// As above. Everywhere else a path is not a byte string, and `text/uri-list`
/// is defined as UTF-8, so anything else is rejected rather than mangled.
#[cfg(not(unix))]
fn bytes_to_path(bytes: Vec<u8>) -> Option<PathBuf> {
    String::from_utf8(bytes).ok().map(PathBuf::from)
}

/// `[u8]::trim_ascii`, plus the CR of a CRLF.
const fn trim_ascii(mut line: &[u8]) -> &[u8] {
    while let [first, rest @ ..] = line {
        if first.is_ascii_whitespace() {
            line = rest;
        } else {
            break;
        }
    }
    while let [rest @ .., last] = line {
        if last.is_ascii_whitespace() {
            line = rest;
        } else {
            break;
        }
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_engine_mimes_have_the_specified_spellings() {
        // These strings are a compatibility contract with every other
        // application on the desktop and with older builds of this engine.
        assert_eq!(MimeType::TextUtf8.as_str(), "text/plain;charset=utf-8");
        assert_eq!(MimeType::CrcblRon.as_str(), "application/x-crcbl+ron");
        assert_eq!(MimeType::UriList.as_str(), "text/uri-list");
        assert_eq!(MimeType::Other("image/png").to_string(), "image/png");
    }

    #[test]
    fn the_common_plain_text_spellings_are_all_accepted() {
        for spelling in [
            "text/plain;charset=utf-8",
            "text/plain",
            "UTF8_STRING",
            "STRING",
        ] {
            assert_eq!(
                MimeType::recognize(spelling),
                Some(MimeType::TextUtf8),
                "{spelling}"
            );
        }
        assert_eq!(MimeType::recognize("text/html"), None);
        assert_eq!(
            MimeType::recognize("application/x-crcbl+ron"),
            Some(MimeType::CrcblRon)
        );
        assert_eq!(
            MimeType::recognize("text/uri-list"),
            Some(MimeType::UriList)
        );
    }

    #[test]
    fn a_received_mime_holds_what_the_other_application_actually_said() {
        // The hole this type closes: `MimeType::Other` is `&'static str` and
        // cannot hold a string that arrived at runtime.
        let foreign = ReceivedMime::new("application/vnd.some-editor.scene+json");
        assert_eq!(foreign.recognized(), None);
        assert_eq!(
            foreign.as_str(),
            "application/vnd.some-editor.scene+json",
            "the literal spelling survives"
        );
        assert_eq!(foreign.to_string(), foreign.as_str());
        assert!(!foreign.matches(MimeType::TextUtf8));

        // An alternative spelling is preserved verbatim *and* recognized.
        let plain = ReceivedMime::new("text/plain");
        assert_eq!(plain.as_str(), "text/plain", "no canonicalization");
        assert_eq!(plain.recognized(), Some(MimeType::TextUtf8));
        assert!(
            plain.matches(MimeType::TextUtf8),
            "a paste handler asking for text must accept this"
        );
        assert_ne!(plain, ReceivedMime::from(MimeType::TextUtf8));

        // The engine's own formats round-trip without allocating a new string.
        for known in [MimeType::TextUtf8, MimeType::CrcblRon, MimeType::UriList] {
            let received = ReceivedMime::new(known.as_str());
            assert_eq!(received, ReceivedMime::from(known));
            assert_eq!(received.recognized(), Some(known));
            assert!(received.matches(known));
        }
    }

    #[test]
    fn a_uri_list_is_crlf_separated_percent_encoded_and_may_carry_comments() {
        // What a file manager actually sends for a two-file drop, byte for
        // byte: a comment line the format permits, CRLF endings, and a space
        // encoded as `%20`.
        let dropped = b"#comment\r\n\
                        file:///home/dev/My%20Scene.ron\r\n\
                        file:///tmp/a%2Bb%2Fc.png\r\n";
        assert_eq!(
            parse_uri_list(dropped),
            vec![
                PathBuf::from("/home/dev/My Scene.ron"),
                // `%2F` is a slash *in the name*, decoded after the URI has
                // been split — decoding first would invent a directory.
                PathBuf::from("/tmp/a+b/c.png"),
            ]
        );

        // Bare LF, no trailing newline, and leading whitespace: senders do all
        // three, and insisting on the letter of the spec would return nothing.
        assert_eq!(
            parse_uri_list(b"  file:///tmp/one\nfile:///tmp/two"),
            vec![PathBuf::from("/tmp/one"), PathBuf::from("/tmp/two")]
        );
        assert!(parse_uri_list(b"").is_empty());
        assert!(parse_uri_list(b"\r\n\r\n# only comments\r\n").is_empty());
    }

    #[test]
    fn a_uri_that_is_not_a_local_file_is_not_turned_into_a_path() {
        // The decision this function is most likely to get wrong: a URL is not
        // a path, and producing `example.com/x` from one would look like it
        // worked until something opened it.
        for hostile in [
            &b"https://example.com/x"[..],
            b"trash:///deleted",
            b"smb://server/share/file",
            b"file://remotehost/etc/passwd",
            b"file:relative/path",
            b"file://",
            b"not a uri at all",
        ] {
            assert!(
                parse_uri_list(hostile).is_empty(),
                "{}",
                String::from_utf8_lossy(hostile)
            );
        }

        // An empty authority and an explicit `localhost` are both this machine,
        // and `file:/path` is RFC 8089's authority-less form.
        assert_eq!(
            parse_uri_list(b"FILE:///tmp/x\nfile://localhost/tmp/y\nfile:/tmp/z"),
            vec![
                PathBuf::from("/tmp/x"),
                PathBuf::from("/tmp/y"),
                PathBuf::from("/tmp/z"),
            ],
            "the scheme is case-insensitive and the authority may be omitted"
        );
        assert_eq!(
            parse_uri_list(b"file:///"),
            vec![PathBuf::from("/")],
            "the root directory is a real path, degenerate as a drop is"
        );

        // A stray `%` is a literal `%`, not a reason to lose the file.
        assert_eq!(
            parse_uri_list(b"file:///tmp/100%%20done"),
            vec![PathBuf::from("/tmp/100% done")]
        );
        assert_eq!(
            parse_uri_list(b"file:///tmp/trailing%"),
            vec![PathBuf::from("/tmp/trailing%")]
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_path_that_is_not_utf8_survives_the_round_trip() {
        // Unix filenames are bytes. A backend that went through `String` would
        // replace this one with U+FFFD and hand back a path that does not
        // exist.
        use std::os::unix::ffi::OsStrExt;
        let paths = parse_uri_list(b"file:///tmp/%FF%FE.bin");
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].as_os_str().as_bytes(), b"/tmp/\xff\xfe.bin");
    }

    #[test]
    fn offers_carry_bytes_without_copying() {
        let offer = ClipboardOffer::text("hello");
        assert_eq!(offer.mime, MimeType::TextUtf8);
        assert_eq!(offer.bytes, b"hello");
        assert_eq!(ClipboardOffer::ron("(x:1)").mime, MimeType::CrcblRon);
        assert_eq!(ClipboardRequestId(2).to_string(), "clipboard request 2");
    }
}
