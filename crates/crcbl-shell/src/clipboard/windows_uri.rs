//! `file:` URIs for Windows paths, in both directions.
//!
//! [`parse_uri_list`](super::parse_uri_list)'s POSIX decoder maps a URI's path
//! onto a path byte for byte, which is right wherever `/` is the root and wrong
//! here in two ways: `file:///C:/a` names `C:\a`, not `/C:/a`, and
//! `file://server/share/x` names the UNC path `\\server\share\x` rather than a
//! file on a host this machine cannot reach. Both mappings are RFC 8089's —
//! the drive letter as the first path segment (section 2 and appendix E.2), and
//! the UNC host as the URI's authority (appendix E.3.1) — and they are what
//! Explorer, the browsers and every file manager on the desktop exchange.
//!
//! # Decision: a separate pair, not a mode of the POSIX decoder
//!
//! The two agree on the scheme and on percent-decoding and on nothing else: one
//! keeps a non-local authority out, the other turns it into a server name; one
//! takes path bytes verbatim, the other has to find a drive, reject a name that
//! decodes to a separator and turn `/` into `\`. A flag through one function
//! would be two functions sharing an argument. What *is* shared —
//! RFC 3986 percent-decoding — is [`super::percent_decode`], called rather
//! than copied.
//!
//! Both halves are pure and operate on strings, not on [`std::path::Path`], so
//! they are unit-tested on every host even though only the Win32 backend and
//! `parse_uri_list` on Windows call them.
//!
//! # What is encoded
//!
//! Every byte outside RFC 3986's `unreserved` set (`A–Z a–z 0–9 - . _ ~`) is
//! percent-encoded, uppercase hex, in the path segments and in the host alike.
//! That is stricter than the grammar needs — `pchar` also admits the
//! sub-delimiters, `:` and `@` — and strictness is the safe side: an escaped
//! `&` decodes identically everywhere, whereas a literal `#` or `?` ends the
//! path for any conforming reader. Non-ASCII names are their UTF-8 bytes,
//! escaped, which is what RFC 8089 section 4 and RFC 3986 section 2.5
//! prescribe.
//! The one byte left literal outside `unreserved` is the drive letter's `:`,
//! because every reader looks for `C:` and not all of them decode `C%3A`.

use super::percent_decode;

/// A Windows path as a `file:` URI, or `None` when it names no file a URI can.
///
/// Accepted:
///
/// * a drive-letter path, `C:\a b\c` → `file:///C:/a%20b/c`;
/// * a UNC path, `\\server\share\x` → `file://server/share/x`;
/// * either one in its `\\?\` verbatim spelling (`\\?\C:\…`, `\\?\UNC\…`),
///   which is how a long path arrives.
///
/// `/` is accepted as a separator anywhere `\` is, as Windows itself does.
/// Refused: a relative path, a drive-relative `C:x`, a rooted `\x` with no
/// drive, and a `\\.\` device path — none of them is a location another process
/// can resolve the same way.
#[must_use]
pub(crate) fn windows_path_to_file_uri(path: &str) -> Option<String> {
    let path = path.strip_prefix(r"\\?\").map_or_else(
        || path.to_string(),
        |verbatim| {
            verbatim
                .strip_prefix(r"UNC\")
                .map_or_else(|| verbatim.to_string(), |unc| format!(r"\\{unc}"))
        },
    );
    let is_separator = |c: char| c == '\\' || c == '/';
    let mut uri = String::from("file://");

    let rest = if let Some(unc) = path
        .strip_prefix(['\\', '/'])
        .and_then(|p| p.strip_prefix(['\\', '/']))
    {
        let split = unc.find(is_separator)?;
        let (server, rest) = unc.split_at(split);
        let share = rest[1..].split(is_separator).next().unwrap_or("");
        if server.is_empty() || server == "." || share.is_empty() {
            return None;
        }
        percent_encode_into(&mut uri, server);
        rest
    } else {
        let bytes = path.as_bytes();
        let has_drive = bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && is_separator(char::from(bytes[2]));
        if !has_drive {
            return None;
        }
        uri.push('/');
        uri.push_str(&path[..2]);
        &path[2..]
    };

    for (index, segment) in rest.split(is_separator).enumerate() {
        if index > 0 {
            uri.push('/');
        }
        percent_encode_into(&mut uri, segment);
    }
    Some(uri)
}

/// A `file:` URI as the Windows path it names, or `None` when it names none.
///
/// The inverse of [`windows_path_to_file_uri`], and more forgiving on input,
/// because other applications write these:
///
/// * `file:///C:/x`, `file://localhost/C:/x`, `file:/C:/x` and `file:C:/x`
///   all name `C:\x` — the last two are RFC 8089 appendix E.2's
///   authority-less forms — and so does the legacy `C|` drive spelling;
/// * `file://server/share/x` names `\\server\share\x`, and so does the
///   four-slash `file:////server/share/x` of appendix E.3.2;
/// * the scheme is case-insensitive, and the path ends at a `?` or `#`, which
///   RFC 3986 section 3.3 makes a query or fragment delimiter rather than part
///   of any name — an encoder that meant the character escapes it.
///
/// Each segment is percent-decoded **after** the URI has been split on `/`,
/// and a segment that decodes to `\`, `/` or NUL is refused rather than turned
/// into a separator: no Windows name contains one, so the URI names no file.
/// So is a URI whose path does not start at a drive or a share —
/// `file:///tmp/x` is a POSIX path, and there is no drive to resolve it
/// against.
#[must_use]
pub(crate) fn file_uri_to_windows_path(uri: &[u8]) -> Option<String> {
    const SCHEME: &[u8] = b"file:";
    let (prefix, rest) = uri.split_at_checked(SCHEME.len())?;
    if !prefix.eq_ignore_ascii_case(SCHEME) {
        return None;
    }
    let end = rest
        .iter()
        .position(|byte| matches!(byte, b'?' | b'#'))
        .unwrap_or(rest.len());
    let rest = &rest[..end];

    let (host, path) = match rest.strip_prefix(b"//") {
        Some(authority) => split_authority(authority),
        None => (&b""[..], rest),
    };
    let (host, path) = if host.is_empty() || host.eq_ignore_ascii_case(b"localhost") {
        match path.strip_prefix(b"//") {
            // `file:////server/share` — an empty authority and a UNC path.
            Some(unc) => split_authority(unc),
            None => (&b""[..], path),
        }
    } else {
        (host, path)
    };

    if host.is_empty() {
        let mut segments = path
            .strip_prefix(b"/")
            .unwrap_or(path)
            .split(|b| *b == b'/');
        let drive = decode_segment(segments.next()?)?;
        let letter = match drive.as_bytes() {
            [letter, b':' | b'|'] if letter.is_ascii_alphabetic() => char::from(*letter),
            _ => return None,
        };
        let mut windows = format!("{letter}:");
        let mut any = false;
        for segment in segments {
            windows.push('\\');
            windows.push_str(&decode_segment(segment)?);
            any = true;
        }
        if !any {
            // `file:///C:` is the drive's root; `C:` alone would be the
            // current directory on that drive, which is not what it names.
            windows.push('\\');
        }
        return Some(windows);
    }

    let server = decode_segment(host)?;
    if server.contains(':') {
        // A port, or an IP literal's colons: neither is a UNC server name.
        return None;
    }
    // The path is empty or starts at a `/`, so its first piece is empty.
    let mut segments = path.split(|b| *b == b'/').skip(1);
    // `file://server` and `file://server/` name no share.
    let share = decode_segment(segments.next()?)?;
    if share.is_empty() {
        return None;
    }
    let mut windows = format!(r"\\{server}\{share}");
    for segment in segments {
        windows.push('\\');
        windows.push_str(&decode_segment(segment)?);
    }
    Some(windows)
}

/// RFC 2483 `text/uri-list` naming `paths`: one URI per line, each line
/// CRLF-terminated.
///
/// A path [`windows_path_to_file_uri`] refuses is left out and logged, the
/// same way [`parse_uri_list`](super::parse_uri_list) skips a URI that names no
/// local file: one unnameable entry does not cost the rest of the list.
#[must_use]
pub(crate) fn uri_list_from_windows_paths<'a>(paths: impl IntoIterator<Item = &'a str>) -> String {
    let mut list = String::new();
    for path in paths {
        match windows_path_to_file_uri(path) {
            Some(uri) => {
                list.push_str(&uri);
                list.push_str("\r\n");
            }
            None => crcbl_core::log::warn!(
                "{path:?} has no file: URI, so it is left out of the text/uri-list"
            ),
        }
    }
    list
}

/// `file://<authority>/<path>` split at the authority's end, which is the first
/// `/` — also the first byte of the path — or the end of the URI.
fn split_authority(authority: &[u8]) -> (&[u8], &[u8]) {
    let split = authority
        .iter()
        .position(|byte| *byte == b'/')
        .unwrap_or(authority.len());
    authority.split_at(split)
}

/// One percent-decoded path segment, or `None` if it is not UTF-8 or decodes
/// to a byte no Windows name can hold in that position.
fn decode_segment(segment: &[u8]) -> Option<String> {
    let decoded = String::from_utf8(percent_decode(segment)).ok()?;
    (!decoded.contains(['\\', '/', '\0'])).then_some(decoded)
}

/// Appends `text` with every byte outside RFC 3986's `unreserved` set escaped.
fn percent_encode_into(out: &mut String, text: &str) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 0x0F)]));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pairs whose URI side is written out by hand from RFC 8089 and RFC 3986,
    /// not produced by the encoder, so the encoder is checked against the
    /// specification rather than against itself.
    const KNOWN: &[(&str, &str)] = &[
        (r"C:\a b\c", "file:///C:/a%20b/c"),
        (r"C:\", "file:///C:/"),
        (r"d:\x.txt", "file:///d:/x.txt"),
        (r"\\server\share\x", "file://server/share/x"),
        (r"\\server\share", "file://server/share"),
        (r"\\my-nas.local\p q\r", "file://my-nas.local/p%20q/r"),
        // The reserved characters a Windows name can hold, each escaped:
        // `#` would start a fragment, `%` an escape, and `?` a query (it is
        // not legal in a name, but a URI still has to say it unambiguously).
        (r"C:\100% #1\a&b", "file:///C:/100%25%20%231/a%26b"),
        // UTF-8, byte by byte: `é` is C3 A9, `プ` is E3 83 97, and the
        // astral `🎮` (U+1F3AE) is F0 9F 8E AE.
        (
            r"C:\café\プ\🎮.png",
            "file:///C:/caf%C3%A9/%E3%83%97/%F0%9F%8E%AE.png",
        ),
        (r"C:\a~b_c-d.e", "file:///C:/a~b_c-d.e"),
    ];

    #[test]
    fn known_paths_encode_to_the_uris_the_rfcs_give() {
        for (path, uri) in KNOWN {
            assert_eq!(
                windows_path_to_file_uri(path).as_deref(),
                Some(*uri),
                "{path}"
            );
        }
    }

    #[test]
    fn known_uris_decode_to_the_paths_they_name() {
        for (path, uri) in KNOWN {
            assert_eq!(
                file_uri_to_windows_path(uri.as_bytes()).as_deref(),
                Some(*path),
                "{uri}"
            );
        }
    }

    #[test]
    fn every_known_path_round_trips() {
        for (path, _) in KNOWN {
            let uri = windows_path_to_file_uri(path).expect("a known path encodes");
            assert_eq!(
                file_uri_to_windows_path(uri.as_bytes()).as_deref(),
                Some(*path),
                "through {uri}"
            );
        }
    }

    #[test]
    fn the_verbatim_and_forward_slash_spellings_name_the_same_files() {
        // `\\?\` is how a path past MAX_PATH arrives, and `/` is a separator
        // everywhere a Windows API takes a path. Neither changes the file.
        for (spelling, uri) in [
            (r"\\?\C:\long\x", "file:///C:/long/x"),
            (r"\\?\UNC\server\share\x", "file://server/share/x"),
            ("C:/a b/c", "file:///C:/a%20b/c"),
            ("//server/share/x", "file://server/share/x"),
        ] {
            assert_eq!(
                windows_path_to_file_uri(spelling).as_deref(),
                Some(uri),
                "{spelling}"
            );
        }
    }

    #[test]
    fn a_path_that_names_no_resolvable_file_has_no_uri() {
        for path in [
            "",
            "relative\\x",
            r"C:relative",
            "C:",
            r"\rooted\without\drive",
            r"\\.\pipe\name",
            r"\\server",
            r"\\server\",
            r"\\\share",
        ] {
            assert_eq!(windows_path_to_file_uri(path), None, "{path:?}");
        }
    }

    #[test]
    fn the_forms_other_applications_write_decode_too() {
        for (uri, path) in [
            ("FILE:///C:/x", r"C:\x"),
            ("file://localhost/C:/x", r"C:\x"),
            ("file://LOCALHOST/C:/x", r"C:\x"),
            ("file:/C:/x", r"C:\x"),
            ("file:C:/x", r"C:\x"),
            ("file:///C|/x", r"C:\x"),
            ("file:///C%3A/x", r"C:\x"),
            ("file:///C:", r"C:\"),
            ("file:////server/share/x", r"\\server\share\x"),
            ("file:///C:/dir/", r"C:\dir\"),
            // Lower-case escapes are as valid as upper-case ones.
            ("file:///C:/caf%c3%a9", r"C:\café"),
            // A query or fragment is not part of the name.
            ("file:///C:/a%23b#fragment", r"C:\a#b"),
            ("file:///C:/a?query", r"C:\a"),
        ] {
            assert_eq!(
                file_uri_to_windows_path(uri.as_bytes()).as_deref(),
                Some(path),
                "{uri}"
            );
        }
    }

    #[test]
    fn a_uri_that_names_no_windows_file_decodes_to_nothing() {
        for uri in [
            // A POSIX path has no drive to resolve against.
            "file:///tmp/x",
            "file:///",
            "file://",
            "https://example.com/C:/x",
            "file:relative/x",
            // A drive "letter" that is not one.
            "file:///1:/x",
            "file:///CD:/x",
            // A segment that decodes to a separator or NUL would invent a
            // directory, or truncate the name at the first system call.
            "file:///C:/a%5Cb",
            "file:///C:/a%2Fb",
            "file:///C:/a%00b",
            "file://server%2Fevil/share",
            // Not UTF-8 once decoded.
            "file:///C:/%FF",
            // No share, and a host with a port.
            "file://server",
            "file://server/",
            "file://server//x",
            "file://server:445/share",
        ] {
            assert_eq!(file_uri_to_windows_path(uri.as_bytes()), None, "{uri}");
        }
    }

    #[test]
    fn a_uri_list_is_one_crlf_terminated_line_per_nameable_path() {
        assert_eq!(
            uri_list_from_windows_paths([r"C:\a b", "relative", r"\\s\h\ü"]),
            "file:///C:/a%20b\r\nfile://s/h/%C3%BC\r\n",
            "the relative path is left out, and every line ends in CRLF"
        );
        assert_eq!(uri_list_from_windows_paths([]), "");
    }
}
