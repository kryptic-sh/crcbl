//! The one kind of URL [`Shell::open_url`](crate::Shell::open_url) hands to
//! the system: an absolute `http` or `https` URL, checked before any backend
//! sees it.
//!
//! # Decision: a type, not a string the backends each check
//!
//! What a platform does with a string it is told to "open" is far wider than
//! "show a web page". `ShellExecuteW` with the `open` verb runs an executable
//! (`C:\x.exe`), opens any file with its associated program (`\\server\share\
//! payload.lnk`), and starts any registered protocol handler (`ms-msdt:`,
//! `search-ms:`, `file:`), each of which has been a remote-code-execution path
//! in its own right. `xdg-open` and `NSWorkspace openURL` are as broad. A game
//! that passes on a link it was sent — a server's news feed, a mod's homepage,
//! a chat message — must never reach any of that, so the check cannot be left
//! to each backend to remember. [`WebUrl`] is the only thing
//! [`open_url`](crate::Shell::open_url) takes, and [`WebUrl::parse`] is the
//! only way to make one, so a string that fails the check never reaches a
//! system call on any platform.
//!
//! # What is accepted
//!
//! RFC 3986's `URI` with the scheme `http` or `https` (in any case, as
//! section 3.1 says schemes are) and an authority: `//`, a host, and an
//! optional port. Stricter than the grammar in four ways, each on the side of
//! refusing:
//!
//! * **Only the characters RFC 3986 lets a URI contain** (section 2): the
//!   unreserved set, the delimiters, and `%` followed by two hex digits.
//!   Everything else must arrive percent-encoded, which turns away whitespace
//!   at either end or inside, NUL and every other control character, the `"`
//!   that would end a quoted argument on the browser's command line, the `\`
//!   that Windows reads as a separator, and non-ASCII text.
//! * **No user information.** `https://bank.example@evil.example` names the
//!   host after the `@`, which is how a link is made to read as one site and
//!   go to another; a game has no use for a password in a URL either.
//! * **`[` and `]` only around an IPv6 address literal** in the host, as
//!   section 3.2.2 says, and no `IPvFuture` literal.
//! * **A port is one to five digits, at most 65535**, where the grammar allows
//!   any run of digits or none.
//!
//! The string is kept exactly as given: nothing is decoded, normalised or
//! lower-cased, so what the browser opens is what the caller passed.

use core::fmt;

/// An absolute `http` or `https` URL, the only kind
/// [`Shell::open_url`](crate::Shell::open_url) opens.
///
/// Made by [`parse`](Self::parse); see the [module docs](self) for what it
/// accepts and why it is this strict.
///
/// ```
/// use crcbl_shell::{WebUrl, WebUrlError};
///
/// let url = WebUrl::parse("https://store.example/app/1234?ref=menu")?;
/// assert_eq!(url.as_str(), "https://store.example/app/1234?ref=menu");
///
/// assert_eq!(WebUrl::parse(r"C:\game\patch.exe"), Err(WebUrlError::Character {
///     found: '\\',
///     at: 2,
/// }));
/// assert_eq!(WebUrl::parse("file:///C:/game/patch.exe"), Err(WebUrlError::Scheme));
/// # Ok::<(), WebUrlError>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct WebUrl(String);

/// Why a string is not a [`WebUrl`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum WebUrlError {
    /// A character a URL may only carry percent-encoded: whitespace, a
    /// control character, `"`, `\`, `<`, `>`, `^`, `` ` ``, `{`, `|`, `}`,
    /// anything outside ASCII, or a `[` or `]` outside the host.
    #[error("{found:?} at byte {at} must be percent-encoded in a URL")]
    Character {
        /// The character.
        found: char,
        /// Its byte offset in the string.
        at: usize,
    },

    /// A `%` not followed by two hex digits.
    #[error("the % at byte {at} does not start a percent escape")]
    Escape {
        /// The `%`'s byte offset in the string.
        at: usize,
    },

    /// The scheme is missing or is not `http` or `https`.
    #[error("only http and https URLs can be opened")]
    Scheme,

    /// No `//` after the scheme, an empty host, or a malformed IPv6 literal.
    #[error("the URL has no host, or a malformed one")]
    Host,

    /// A user name or password before the host.
    #[error("a URL with a user name or password before the host is refused")]
    UserInfo,

    /// A port that is empty, not decimal, or above 65535.
    #[error("the URL's port is not a number from 0 to 65535")]
    Port,
}

impl WebUrl {
    /// `text` as a [`WebUrl`], if it is an absolute `http` or `https` URL that
    /// passes every check in the [module docs](self).
    ///
    /// # Errors
    ///
    /// The first check `text` fails, as a [`WebUrlError`].
    pub fn parse(text: &str) -> Result<Self, WebUrlError> {
        check_characters(text)?;
        let (scheme, rest) = text.split_once(':').ok_or(WebUrlError::Scheme)?;
        if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
            return Err(WebUrlError::Scheme);
        }
        let rest = rest.strip_prefix("//").ok_or(WebUrlError::Host)?;
        let (authority, tail) = rest.split_at(rest.find(['/', '?', '#']).unwrap_or(rest.len()));
        check_authority(authority)?;
        let tail_start = text.len() - tail.len();
        if let Some((offset, found)) = tail.char_indices().find(|&(_, c)| c == '[' || c == ']') {
            return Err(WebUrlError::Character {
                found,
                at: tail_start + offset,
            });
        }
        Ok(Self(text.to_owned()))
    }

    /// The URL, exactly as it was parsed.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WebUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Every character is one RFC 3986 section 2 lets a URI contain, and every `%`
/// begins an escape.
fn check_characters(text: &str) -> Result<(), WebUrlError> {
    let bytes = text.as_bytes();
    for (at, found) in text.char_indices() {
        match found {
            '%' => {
                let escape = bytes.get(at + 1..at + 3);
                if !escape.is_some_and(|digits| digits.iter().all(u8::is_ascii_hexdigit)) {
                    return Err(WebUrlError::Escape { at });
                }
            }
            'A'..='Z' | 'a'..='z' | '0'..='9' => {}
            // Unreserved, then the general and the sub-delimiters.
            '-' | '.' | '_' | '~' => {}
            ':' | '/' | '?' | '#' | '[' | ']' | '@' => {}
            '!' | '$' | '&' | '\'' | '(' | ')' | '*' | '+' | ',' | ';' | '=' => {}
            _ => return Err(WebUrlError::Character { found, at }),
        }
    }
    Ok(())
}

/// The authority is a non-empty host, a reg-name or a bracketed IPv6 literal,
/// with an optional port and no user information. Its characters have already
/// been checked, and `/`, `?` and `#` cannot be in it, since they end it.
fn check_authority(authority: &str) -> Result<(), WebUrlError> {
    if authority.contains('@') {
        return Err(WebUrlError::UserInfo);
    }
    let port = if let Some(literal) = authority.strip_prefix('[') {
        let (address, after) = literal.split_once(']').ok_or(WebUrlError::Host)?;
        let is_ipv6 = address.contains(':')
            && address
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() || byte == b':' || byte == b'.');
        if !is_ipv6 {
            return Err(WebUrlError::Host);
        }
        if after.is_empty() {
            None
        } else {
            Some(after.strip_prefix(':').ok_or(WebUrlError::Host)?)
        }
    } else {
        if authority.contains(['[', ']']) {
            return Err(WebUrlError::Host);
        }
        let (host, port) = authority
            .split_once(':')
            .map_or((authority, None), |(host, port)| (host, Some(port)));
        if host.is_empty() {
            return Err(WebUrlError::Host);
        }
        port
    };
    match port {
        None => Ok(()),
        Some(port)
            if (1..=5).contains(&port.len())
                && port.bytes().all(|byte| byte.is_ascii_digit())
                && port.parse::<u32>().is_ok_and(|number| number <= 65535) =>
        {
            Ok(())
        }
        Some(_) => Err(WebUrlError::Port),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused(text: &str) -> WebUrlError {
        WebUrl::parse(text).expect_err(text)
    }

    #[test]
    fn web_urls_are_accepted_verbatim() {
        for text in [
            "http://example.com",
            "https://example.com/",
            "https://store.example/app/1234?ref=menu&x=1#reviews",
            "HTTPS://EXAMPLE.COM/PATH",
            "HtTp://example.com",
            "https://example.com:443/",
            "http://example.com:0",
            "https://example.com:65535/x",
            "https://127.0.0.1:8080/",
            "https://[::1]/",
            "https://[2001:db8::7]:8443/a?b#c",
            "https://[::ffff:192.0.2.1]",
            "https://example.com/a%20b/%C3%A9?q=%2F",
            "https://example.com?query",
            "https://example.com#fragment",
            "https://example.com/!$&'()*+,;=:@-._~",
            "https://xn--bcher-kva.example/",
        ] {
            let url = WebUrl::parse(text).unwrap_or_else(|error| panic!("{text}: {error}"));
            assert_eq!(url.as_str(), text, "kept as given");
            assert_eq!(url.to_string(), text);
        }
    }

    #[test]
    fn other_schemes_are_refused() {
        for text in [
            "file:///C:/Windows/System32/calc.exe",
            "FILE://server/share/x.lnk",
            "javascript:alert(1)",
            "ms-msdt:/id",
            "search-ms:query=x",
            "shell:startup",
            "about:blank",
            "ftp://example.com/",
            "httpx://example.com",
            "https+x://example.com",
            "data:text/html,x",
            "mailto:someone@example.com",
            "C:/game/patch.exe",
            "://example.com",
            "",
            "example.com",
            "/usr/bin/xterm",
            "relative/path",
        ] {
            assert_eq!(refused(text), WebUrlError::Scheme, "{text}");
        }
    }

    #[test]
    fn a_web_scheme_needs_a_host() {
        for text in [
            "https:example.com",
            "https:/example.com",
            "https:",
            "https://",
            "https:///path",
            "http://?q",
            "http://#f",
            "https://:443/",
        ] {
            assert_eq!(refused(text), WebUrlError::Host, "{text}");
        }
    }

    #[test]
    fn characters_a_url_cannot_carry_are_refused_where_they_are() {
        for (text, found, at) in [
            (" https://example.com", ' ', 0),
            ("https://example.com ", ' ', 19),
            ("https://example.com\n", '\n', 19),
            ("\thttps://example.com", '\t', 0),
            ("https://exa\0mple.com", '\0', 11),
            ("https://example.com/\0.exe", '\0', 20),
            ("https://example.com/a b", ' ', 21),
            ("https://example.com/\"--gpu-launcher", '"', 20),
            (r"https://good.example\@evil.example", '\\', 20),
            (r"C:\x.exe", '\\', 2),
            (r"\\server\share\payload.exe", '\\', 0),
            ("https://exämple.com", 'ä', 10),
            ("https://example.com/\u{202e}exe.txt", '\u{202e}', 20),
            ("https://example.com/<script>", '<', 20),
            ("https://example.com/a|b", '|', 21),
            ("https://example.com/{x}", '{', 20),
            ("https://example.com/`x`", '`', 20),
            ("https://example.com/a^b", '^', 21),
            ("https://example.com/[x]", '[', 20),
            ("https://example.com/?q=]", ']', 23),
            ("https://example.com#[", '[', 20),
        ] {
            assert_eq!(
                refused(text),
                WebUrlError::Character { found, at },
                "{text:?}"
            );
        }
    }

    #[test]
    fn a_percent_must_begin_an_escape() {
        for (text, at) in [
            ("https://example.com/%zz", 20),
            ("https://example.com/%4", 20),
            ("https://example.com/%", 20),
            ("https://example.com/%%41", 20),
            ("https://ex%2", 10),
        ] {
            assert_eq!(refused(text), WebUrlError::Escape { at }, "{text}");
        }
    }

    #[test]
    fn user_information_is_refused() {
        for text in [
            "https://user@example.com/",
            "https://user:password@example.com/",
            "https://bank.example@evil.example/",
            "https://@example.com",
        ] {
            assert_eq!(refused(text), WebUrlError::UserInfo, "{text}");
        }
    }

    #[test]
    fn a_port_is_a_number_a_port_can_be() {
        for text in [
            "https://example.com:/",
            "https://example.com:65536",
            "https://example.com:99999",
            "https://example.com:000080",
            "https://example.com:8a",
            "https://example.com:+80",
            "https://example.com:80:80",
            "https://[::1]:",
            "https://[::1]:x",
        ] {
            assert_eq!(refused(text), WebUrlError::Port, "{text}");
        }
    }

    #[test]
    fn brackets_are_only_an_ipv6_literal() {
        for text in [
            "https://[::1",
            "https://[]/",
            "https://[example.com]/",
            "https://[v1.x]/",
            "https://[127.0.0.1]/",
            "https://[::1]x/",
            "https://a]b/",
            "https://a[b/",
            "https://example.com:[80]",
        ] {
            assert_eq!(refused(text), WebUrlError::Host, "{text}");
        }
    }

    #[test]
    fn errors_say_what_is_wrong() {
        assert_eq!(
            refused("https://example.com/a b").to_string(),
            "' ' at byte 21 must be percent-encoded in a URL"
        );
        assert_eq!(
            refused("file:///x").to_string(),
            "only http and https URLs can be opened"
        );
    }
}
