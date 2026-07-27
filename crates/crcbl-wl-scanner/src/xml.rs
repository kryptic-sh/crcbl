//! A deliberately small XML reader, sized to the Wayland protocol dialect.
//!
//! # Why this exists rather than a dependency
//!
//! Wayland protocol files are XML in the narrowest sense: a fixed element
//! vocabulary (`protocol`, `interface`, `request`, `event`, `enum`, `entry`,
//! `arg`, `description`, `copyright`), attributes that are all plain strings,
//! one level of comments, an XML declaration, and no namespaces, no CDATA, no
//! entity declarations, no DTD subset, no processing instructions beyond the
//! declaration. A general parser would be more code *for us* — a dependency in
//! the build graph of every clean engine build — than the reader below.
//!
//! What it deliberately does **not** do: validate, resolve namespaces, or
//! preserve document order of text. Anything it cannot understand is an
//! [`XmlError`] with a byte offset rather than a silent skip, because a
//! protocol file we mis-parse produces a *wrong marshaller*, which fails as
//! memory corruption at runtime rather than as a parse error.

use core::fmt;

/// What the reader hands back, one node at a time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node<'a> {
    /// `<name attr="value" …>` or `<name … />`.
    Start {
        /// Element name.
        name: &'a str,
        /// Attributes in document order, with entities decoded.
        attrs: Vec<(&'a str, String)>,
        /// Whether the tag closed itself (`/>`), so no [`Node::End`] follows.
        empty: bool,
    },
    /// `</name>`.
    End {
        /// Element name.
        name: &'a str,
    },
    /// Character data between tags. Never empty.
    Text {
        /// The decoded text.
        text: String,
    },
}

/// A malformed document, with the byte offset where the reader gave up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XmlError {
    /// Byte offset into the source.
    pub offset: usize,
    /// What was wrong.
    pub message: String,
}

impl fmt::Display for XmlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "malformed XML at byte {}: {}", self.offset, self.message)
    }
}

impl std::error::Error for XmlError {}

/// A pull reader over an XML document.
#[derive(Clone, Debug)]
pub struct Reader<'a> {
    source: &'a str,
    pos: usize,
}

impl<'a> Reader<'a> {
    /// Starts reading `source`.
    #[must_use]
    pub const fn new(source: &'a str) -> Self {
        Self { source, pos: 0 }
    }

    fn error<T>(&self, message: impl Into<String>) -> Result<T, XmlError> {
        Err(XmlError {
            offset: self.pos,
            message: message.into(),
        })
    }

    fn rest(&self) -> &'a str {
        &self.source[self.pos..]
    }

    /// Skips whatever is not a node: comments, the XML declaration, and
    /// doctype declarations. Returns `false` at end of input.
    fn skip_noise(&mut self) -> Result<bool, XmlError> {
        loop {
            let rest = self.rest();
            if rest.is_empty() {
                return Ok(false);
            }
            if let Some(after) = rest.strip_prefix("<!--") {
                let Some(end) = after.find("-->") else {
                    return self.error("unterminated comment");
                };
                self.pos += 4 + end + 3;
            } else if rest.starts_with("<?") {
                let Some(end) = rest.find("?>") else {
                    return self.error("unterminated processing instruction");
                };
                self.pos += end + 2;
            } else if rest.starts_with("<!") {
                // `<!DOCTYPE …>` — no internal subset appears in any Wayland
                // protocol file, so a flat scan to `>` is correct here and a
                // parse error if that ever stops being true.
                let Some(end) = rest.find('>') else {
                    return self.error("unterminated declaration");
                };
                self.pos += end + 1;
            } else {
                return Ok(true);
            }
        }
    }

    /// The next node, or `None` at end of input.
    ///
    /// # Errors
    ///
    /// [`XmlError`] if the document is malformed anywhere the reader looks.
    pub fn next_node(&mut self) -> Result<Option<Node<'a>>, XmlError> {
        if !self.skip_noise()? {
            return Ok(None);
        }
        if !self.rest().starts_with('<') {
            // Character data up to the next tag.
            let rest = self.rest();
            let end = rest.find('<').unwrap_or(rest.len());
            let raw = &rest[..end];
            self.pos += end;
            let text = decode_entities(raw);
            if text.trim().is_empty() {
                return self.next_node();
            }
            return Ok(Some(Node::Text { text }));
        }

        if let Some(after) = self.rest().strip_prefix("</") {
            let Some(end) = after.find('>') else {
                return self.error("unterminated end tag");
            };
            let name = after[..end].trim();
            self.pos += 2 + end + 1;
            return Ok(Some(Node::End { name }));
        }

        // A start tag. Scan to the matching `>`, honouring quoted attribute
        // values so a `>` inside one does not end the tag early.
        let start = self.pos + 1;
        let bytes = self.source.as_bytes();
        let mut index = start;
        let mut quote: Option<u8> = None;
        loop {
            let Some(&byte) = bytes.get(index) else {
                return self.error("unterminated start tag");
            };
            match (quote, byte) {
                (Some(open), byte) if byte == open => quote = None,
                (Some(_), _) => {}
                (None, b'"' | b'\'') => quote = Some(byte),
                (None, b'>') => break,
                (None, _) => {}
            }
            index += 1;
        }
        let inner = &self.source[start..index];
        self.pos = index + 1;

        let (inner, empty) = match inner.strip_suffix('/') {
            Some(stripped) => (stripped, true),
            None => (inner, false),
        };
        let inner = inner.trim();
        let name_end = inner
            .find(|c: char| c.is_ascii_whitespace())
            .unwrap_or(inner.len());
        let name = &inner[..name_end];
        if name.is_empty() {
            return self.error("start tag with no element name");
        }
        let attrs = parse_attributes(&inner[name_end..], start + name_end)?;
        Ok(Some(Node::Start { name, attrs, empty }))
    }
}

/// Splits `input` into `key="value"` pairs.
fn parse_attributes(input: &str, base: usize) -> Result<Vec<(&str, String)>, XmlError> {
    let mut attrs = Vec::new();
    let bytes = input.as_bytes();
    let mut index = 0;
    loop {
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() {
            return Ok(attrs);
        }
        let key_start = index;
        while index < bytes.len() && bytes[index] != b'=' && !bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        let key = &input[key_start..index];
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() || bytes[index] != b'=' {
            return Err(XmlError {
                offset: base + index,
                message: format!("attribute {key:?} has no value"),
            });
        }
        index += 1;
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        let Some(&quote @ (b'"' | b'\'')) = bytes.get(index) else {
            return Err(XmlError {
                offset: base + index,
                message: format!("attribute {key:?} value is not quoted"),
            });
        };
        index += 1;
        let value_start = index;
        while index < bytes.len() && bytes[index] != quote {
            index += 1;
        }
        if index >= bytes.len() {
            return Err(XmlError {
                offset: base + value_start,
                message: format!("attribute {key:?} value is unterminated"),
            });
        }
        attrs.push((key, decode_entities(&input[value_start..index])));
        index += 1;
    }
}

/// Expands the five predefined XML entities and numeric character references.
///
/// Wayland protocol files use `&quot;`, `&amp;`, `&lt;` and `&gt;` in summaries
/// and copyright blocks. An unknown entity is left verbatim rather than
/// dropped: this text never reaches generated code, and silently deleting
/// characters is the worse failure.
fn decode_entities(input: &str) -> String {
    if !input.contains('&') {
        return input.to_string();
    }
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(index) = rest.find('&') {
        out.push_str(&rest[..index]);
        rest = &rest[index..];
        let Some(end) = rest.find(';') else {
            out.push_str(rest);
            return out;
        };
        let entity = &rest[1..end];
        match entity {
            "quot" => out.push('"'),
            "amp" => out.push('&'),
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "apos" => out.push('\''),
            _ => {
                let numeric = entity
                    .strip_prefix("#x")
                    .or_else(|| entity.strip_prefix("#X"))
                    .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                    .or_else(|| entity.strip_prefix('#').and_then(|dec| dec.parse().ok()))
                    .and_then(char::from_u32);
                match numeric {
                    Some(c) => out.push(c),
                    None => out.push_str(&rest[..=end]),
                }
            }
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nodes(source: &str) -> Vec<Node<'_>> {
        let mut reader = Reader::new(source);
        let mut out = Vec::new();
        while let Some(node) = reader.next_node().expect("well-formed") {
            out.push(node);
        }
        out
    }

    #[test]
    fn a_protocol_shaped_document_parses() {
        let source = r#"<?xml version="1.0" encoding="UTF-8"?>
            <!-- a comment with <angles> and "quotes" -->
            <protocol name="wayland">
              <interface name="wl_display" version="1">
                <request name="sync">
                  <arg name="callback" type="new_id" interface="wl_callback"/>
                </request>
              </interface>
            </protocol>"#;
        let nodes = nodes(source);
        assert_eq!(
            nodes[0],
            Node::Start {
                name: "protocol",
                attrs: vec![("name", "wayland".to_string())],
                empty: false,
            }
        );
        assert_eq!(
            nodes[2],
            Node::Start {
                name: "request",
                attrs: vec![("name", "sync".to_string())],
                empty: false,
            }
        );
        let Node::Start { name, attrs, empty } = &nodes[3] else {
            panic!("expected the arg element");
        };
        assert_eq!(*name, "arg");
        assert!(*empty, "a self-closing tag has no matching end tag");
        assert_eq!(attrs[2], ("interface", "wl_callback".to_string()));
        assert_eq!(nodes.last(), Some(&Node::End { name: "protocol" }));
    }

    #[test]
    fn entities_and_quoted_angle_brackets_survive() {
        // `>` inside an attribute value must not end the tag — the failure mode
        // is a truncated summary today and a mis-parsed `interface=` tomorrow.
        let nodes = nodes(r#"<entry name="a" summary="x &gt; y &amp; z" value="&#65;"/>"#);
        let Node::Start { attrs, .. } = &nodes[0] else {
            panic!("expected a start tag");
        };
        assert_eq!(attrs[1].1, "x > y & z");
        assert_eq!(attrs[2].1, "A");
    }

    #[test]
    fn text_between_tags_is_reported_and_whitespace_is_not() {
        let nodes = nodes("<description summary=\"s\">  \n  body text  </description>");
        assert_eq!(nodes.len(), 3, "start, text, end — not a blank text node");
        assert_eq!(
            nodes[1],
            Node::Text {
                text: "  \n  body text  ".to_string()
            }
        );
    }

    #[test]
    fn malformed_input_names_the_offset_rather_than_guessing() {
        let mut reader = Reader::new("<arg name=unquoted/>");
        let error = reader.next_node().expect_err("unquoted value");
        assert!(error.to_string().contains("not quoted"), "{error}");

        let mut reader = Reader::new("<!-- never closed");
        assert!(
            reader
                .next_node()
                .expect_err("unterminated comment")
                .to_string()
                .contains("unterminated comment")
        );

        let mut reader = Reader::new("<interface name=\"x\"");
        assert!(
            reader
                .next_node()
                .expect_err("unterminated tag")
                .to_string()
                .contains("unterminated start tag")
        );
    }
}
