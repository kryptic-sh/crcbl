//! Clipboard and drag-and-drop payloads.
//!
//! # Decision: the trait surface ships now, the backends do not
//!
//! P0.4 implements no platform clipboard — there is no platform code in this
//! slice at all. The *shape* is here anyway, and that is a deliberate trade:
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
//! # Two mime types, always offered together
//!
//! `15-windowing.md` specifies `text/plain` plus the custom
//! `application/x-crcbl+ron` so that engine↔engine copies are lossless while
//! outside applications still receive readable text. Offering both is the
//! caller's job, and [`ClipboardOffer`] is a slice for exactly that reason.

use core::fmt;

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
    fn offers_carry_bytes_without_copying() {
        let offer = ClipboardOffer::text("hello");
        assert_eq!(offer.mime, MimeType::TextUtf8);
        assert_eq!(offer.bytes, b"hello");
        assert_eq!(ClipboardOffer::ron("(x:1)").mime, MimeType::CrcblRon);
        assert_eq!(ClipboardRequestId(2).to_string(), "clipboard request 2");
    }
}
