//! The XML an SVG file is written in, parsed into a tree of elements.
//!
//! Only what a standalone SVG file needs: elements, attributes, character data
//! (text and CDATA sections), the five predefined entities and numeric
//! character references, and namespaces resolved through their `xmlns`
//! declarations. Comments, processing instructions and the document type
//! declaration are skipped.
//!
//! # No DTD
//!
//! A DOCTYPE's internal subset is stepped over, never interpreted: no entity
//! it declares is expanded and no external entity is fetched. That one rule
//! is the whole defence against the "billion laughs" entity bomb and against
//! a file reaching out to the filesystem or the network. A reference to an
//! entity the subset declares is refused as unsupported, so a file that
//! depends on one fails loudly rather than rendering with text missing.
//!
//! # Caps
//!
//! The tree is bounded by [`MAX_DEPTH`] and [`MAX_ELEMENTS`] before anything
//! else sees it, so a file of a few kilobytes cannot nest deeply enough to
//! exhaust the stack of the recursive passes after this one, nor declare more
//! elements than any icon has.

use crate::ImageError;

/// The deepest element nesting accepted. Icons nest a few groups deep; the
/// passes over the tree recurse once per level.
pub(super) const MAX_DEPTH: usize = 128;

/// The most elements a file may hold.
pub(super) const MAX_ELEMENTS: usize = 200_000;

/// The SVG namespace.
pub(super) const SVG_NAMESPACE: &str = "http://www.w3.org/2000/svg";

/// The XLink namespace, which SVG 1.1's `xlink:href` is in.
pub(super) const XLINK_NAMESPACE: &str = "http://www.w3.org/1999/xlink";

/// The namespace the `xml` prefix is bound to without a declaration.
const XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";

/// Which namespace an element or attribute is in, as far as SVG cares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Namespace {
    /// No namespace: an unprefixed attribute, or an element with no default
    /// namespace in scope.
    None,
    /// SVG's own.
    Svg,
    /// XLink's.
    Xlink,
    /// The `xml` prefix's, for `xml:space` and `xml:lang`.
    Xml,
    /// Any other, or a prefix nothing declared: an editor's private data.
    Foreign,
}

/// One attribute, its value with references replaced.
#[derive(Clone, Debug)]
pub(super) struct Attribute {
    pub namespace: Namespace,
    /// The local name, without its prefix.
    pub name: String,
    pub value: String,
}

/// One element.
#[derive(Clone, Debug)]
pub(super) struct Element {
    pub namespace: Namespace,
    /// The local name, without its prefix.
    pub name: String,
    /// Every attribute but the namespace declarations.
    pub attributes: Vec<Attribute>,
    /// Indices of its child elements, in document order.
    pub children: Vec<usize>,
    /// Its direct character data, text and CDATA joined.
    pub text: String,
}

impl Element {
    /// The value of the attribute `name` in `namespace`.
    pub fn attribute(&self, namespace: Namespace, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|attribute| attribute.namespace == namespace && attribute.name == name)
            .map(|attribute| attribute.value.as_str())
    }

    /// Whether it is the SVG element `name`.
    pub fn is(&self, name: &str) -> bool {
        self.namespace == Namespace::Svg && self.name == name
    }
}

/// A parsed file: its elements, the root first.
#[derive(Clone, Debug)]
pub(super) struct Document {
    pub elements: Vec<Element>,
}

impl Document {
    /// The root element.
    pub fn root(&self) -> &Element {
        &self.elements[0]
    }
}

fn malformed(what: impl core::fmt::Display) -> ImageError {
    ImageError::Malformed(format!("SVG XML: {what}"))
}

/// Parses `bytes` as an XML document.
///
/// # Errors
///
/// [`ImageError::Malformed`] for anything that is not well-formed XML in
/// UTF-8, and [`ImageError::Unsupported`] for a compressed (`.svgz`) file, an
/// encoding other than UTF-8, a reference to an entity a DTD declares, or a
/// tree past [`MAX_DEPTH`] or [`MAX_ELEMENTS`].
pub(super) fn parse(bytes: &[u8]) -> Result<Document, ImageError> {
    if bytes.starts_with(&[0x1f, 0x8b]) {
        return Err(ImageError::Unsupported(
            "a gzip-compressed SVG (.svgz)".to_owned(),
        ));
    }
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let text = core::str::from_utf8(bytes)
        .map_err(|error| malformed(format_args!("not UTF-8 ({error})")))?;
    Parser {
        text,
        at: 0,
        elements: Vec::new(),
        open: Vec::new(),
        scopes: Vec::new(),
        dtd_entities: Vec::new(),
    }
    .document()
}

/// One namespace binding in scope: the prefix (empty for the default) and
/// what it is bound to, with the depth of the element that declared it.
struct Binding {
    prefix: String,
    namespace: Namespace,
    depth: usize,
}

struct Parser<'a> {
    text: &'a str,
    at: usize,
    elements: Vec<Element>,
    /// The elements open from the root down.
    open: Vec<usize>,
    scopes: Vec<Binding>,
    /// Entity names a DOCTYPE declared, refused when referenced.
    dtd_entities: Vec<String>,
}

impl Parser<'_> {
    fn rest(&self) -> &str {
        &self.text[self.at..]
    }

    fn starts_with(&self, prefix: &str) -> bool {
        self.rest().starts_with(prefix)
    }

    /// Steps past `prefix`, which must be next.
    fn expect(&mut self, prefix: &str) -> Result<(), ImageError> {
        if self.starts_with(prefix) {
            self.at += prefix.len();
            Ok(())
        } else {
            Err(malformed(format_args!(
                "expected {prefix:?} at byte {}",
                self.at
            )))
        }
    }

    /// Steps to just past the next `end`, returning what came before it.
    fn until(&mut self, end: &str, what: &str) -> Result<&str, ImageError> {
        let start = self.at;
        let Some(offset) = self.rest().find(end) else {
            return Err(malformed(format_args!("{what} at byte {start} never ends")));
        };
        self.at += offset + end.len();
        Ok(&self.text[start..start + offset])
    }

    fn skip_whitespace(&mut self) -> bool {
        let skipped = self
            .rest()
            .bytes()
            .take_while(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
            .count();
        self.at += skipped;
        skipped > 0
    }

    fn name(&mut self) -> Result<&str, ImageError> {
        let start = self.at;
        let length = self
            .rest()
            .char_indices()
            .find(|&(index, c)| !is_name_char(c, index == 0))
            .map_or(self.rest().len(), |(index, _)| index);
        if length == 0 {
            return Err(malformed(format_args!("expected a name at byte {start}")));
        }
        self.at += length;
        Ok(&self.text[start..start + length])
    }

    fn document(mut self) -> Result<Document, ImageError> {
        let mut seen_root = false;
        while self.at < self.text.len() {
            if self.open.is_empty() {
                self.skip_whitespace();
                if self.at == self.text.len() {
                    break;
                }
                if !self.starts_with("<") {
                    return Err(malformed(format_args!(
                        "text outside the root element at byte {}",
                        self.at
                    )));
                }
            }
            if self.starts_with("<?") {
                let body = self.until("?>", "a processing instruction")?.to_owned();
                if body.starts_with("<?xml") {
                    check_declaration(&body, self.text)?;
                }
            } else if self.starts_with("<!--") {
                self.until("-->", "a comment")?;
            } else if self.starts_with("<![CDATA[") {
                self.at += "<![CDATA[".len();
                let data = self.until("]]>", "a CDATA section")?.to_owned();
                self.character_data(&data)?;
            } else if self.starts_with("<!DOCTYPE") {
                if seen_root || !self.open.is_empty() {
                    return Err(malformed("a DOCTYPE after the root element"));
                }
                self.doctype()?;
            } else if self.starts_with("</") {
                self.end_tag()?;
            } else if self.starts_with("<") {
                if self.open.is_empty() && seen_root {
                    return Err(malformed("a second root element"));
                }
                seen_root = true;
                self.start_tag()?;
            } else {
                let length = self.rest().find('<').unwrap_or(self.rest().len());
                let raw = &self.text[self.at..self.at + length];
                self.at += length;
                let decoded = self.unescape(raw)?;
                self.character_data(&decoded)?;
            }
        }
        if let Some(&open) = self.open.last() {
            return Err(malformed(format_args!(
                "<{}> is never closed",
                self.elements[open].name
            )));
        }
        if self.elements.is_empty() {
            return Err(malformed("no root element"));
        }
        Ok(Document {
            elements: self.elements,
        })
    }

    fn character_data(&mut self, data: &str) -> Result<(), ImageError> {
        match self.open.last() {
            Some(&open) => {
                self.elements[open].text.push_str(data);
                Ok(())
            }
            None if data.trim().is_empty() => Ok(()),
            None => Err(malformed("character data outside the root element")),
        }
    }

    /// Steps over a DOCTYPE, recording the names its internal subset declares
    /// as entities and expanding none of them.
    fn doctype(&mut self) -> Result<(), ImageError> {
        let start = self.at;
        self.at += "<!DOCTYPE".len();
        let mut quote = None;
        let mut subset = false;
        while let Some(c) = self.rest().chars().next() {
            if let Some(q) = quote {
                if c == q {
                    quote = None;
                }
                self.at += c.len_utf8();
                continue;
            }
            if subset && self.starts_with("<!--") {
                self.until("-->", "a comment in the DTD")?;
                continue;
            }
            if subset && self.starts_with("<!ENTITY") {
                self.at += "<!ENTITY".len();
                self.skip_whitespace();
                if self.starts_with("%") {
                    // A parameter entity: only the DTD itself can name it.
                    self.at += 1;
                    self.skip_whitespace();
                }
                let name = self.name()?.to_owned();
                self.dtd_entities.push(name);
                continue;
            }
            self.at += c.len_utf8();
            match c {
                '"' | '\'' => quote = Some(c),
                '[' => subset = true,
                ']' => subset = false,
                '>' if !subset => return Ok(()),
                _ => {}
            }
        }
        Err(malformed(format_args!(
            "the DOCTYPE at byte {start} never ends"
        )))
    }

    fn start_tag(&mut self) -> Result<(), ImageError> {
        self.expect("<")?;
        let qualified = self.name()?.to_owned();
        let mut raw_attributes: Vec<(String, String)> = Vec::new();
        let self_closing = loop {
            let spaced = self.skip_whitespace();
            if self.starts_with("/>") {
                self.at += 2;
                break true;
            }
            if self.starts_with(">") {
                self.at += 1;
                break false;
            }
            if !spaced {
                return Err(malformed(format_args!(
                    "expected whitespace before an attribute of <{qualified}> at byte {}",
                    self.at
                )));
            }
            let name = self.name()?.to_owned();
            self.skip_whitespace();
            self.expect("=")?;
            self.skip_whitespace();
            let quote = match self.rest().chars().next() {
                Some(q @ ('"' | '\'')) => q,
                _ => {
                    return Err(malformed(format_args!(
                        "the value of {name} on <{qualified}> is not quoted"
                    )));
                }
            };
            self.at += 1;
            let raw = self.until(if quote == '"' { "\"" } else { "'" }, "an attribute value")?;
            if raw.contains('<') {
                return Err(malformed(format_args!(
                    "'<' in the value of {name} on <{qualified}>"
                )));
            }
            let raw = raw.to_owned();
            // Attribute-value normalisation: each whitespace character becomes
            // a space; references are replaced after, so `&#10;` survives.
            let normalised: String = raw
                .chars()
                .map(|c| {
                    if matches!(c, '\t' | '\r' | '\n') {
                        ' '
                    } else {
                        c
                    }
                })
                .collect();
            let value = self.unescape(&normalised)?;
            if raw_attributes.iter().any(|(seen, _)| *seen == name) {
                return Err(malformed(format_args!(
                    "{name} appears twice on <{qualified}>"
                )));
            }
            raw_attributes.push((name, value));
        };

        let depth = self.open.len() + 1;
        if depth > MAX_DEPTH {
            return Err(ImageError::Unsupported(format!(
                "SVG elements nested more than {MAX_DEPTH} deep"
            )));
        }
        if self.elements.len() >= MAX_ELEMENTS {
            return Err(ImageError::Unsupported(format!(
                "an SVG of more than {MAX_ELEMENTS} elements"
            )));
        }
        for (name, value) in &raw_attributes {
            let prefix = if name == "xmlns" {
                ""
            } else if let Some(prefix) = name.strip_prefix("xmlns:") {
                prefix
            } else {
                continue;
            };
            let namespace = match value.as_str() {
                SVG_NAMESPACE => Namespace::Svg,
                XLINK_NAMESPACE => Namespace::Xlink,
                XML_NAMESPACE => Namespace::Xml,
                "" if prefix.is_empty() => Namespace::None,
                _ => Namespace::Foreign,
            };
            self.scopes.push(Binding {
                prefix: prefix.to_owned(),
                namespace,
                depth,
            });
        }
        let (prefix, local) = split_name(&qualified);
        let namespace = self.resolve(prefix.unwrap_or(""));
        let mut attributes = Vec::with_capacity(raw_attributes.len());
        for (name, value) in raw_attributes {
            if name == "xmlns" || name.starts_with("xmlns:") {
                continue;
            }
            let (prefix, local) = split_name(&name);
            let namespace = match prefix {
                None => Namespace::None,
                Some(prefix) => self.resolve(prefix),
            };
            attributes.push(Attribute {
                namespace,
                name: local.to_owned(),
                value,
            });
        }
        let index = self.elements.len();
        self.elements.push(Element {
            namespace,
            name: local.to_owned(),
            attributes,
            children: Vec::new(),
            text: String::new(),
        });
        if let Some(&parent) = self.open.last() {
            self.elements[parent].children.push(index);
        }
        if self_closing {
            self.close(depth);
        } else {
            self.open.push(index);
        }
        Ok(())
    }

    fn end_tag(&mut self) -> Result<(), ImageError> {
        self.expect("</")?;
        let qualified = self.name()?.to_owned();
        self.skip_whitespace();
        self.expect(">")?;
        let Some(&open) = self.open.last() else {
            return Err(malformed(format_args!("</{qualified}> closes nothing")));
        };
        let (_, local) = split_name(&qualified);
        if self.elements[open].name != local {
            return Err(malformed(format_args!(
                "</{qualified}> closes <{}>",
                self.elements[open].name
            )));
        }
        let depth = self.open.len();
        self.open.pop();
        self.close(depth);
        Ok(())
    }

    /// Drops the namespace bindings the element at `depth` declared.
    fn close(&mut self, depth: usize) {
        while self
            .scopes
            .last()
            .is_some_and(|binding| binding.depth >= depth)
        {
            self.scopes.pop();
        }
    }

    fn resolve(&self, prefix: &str) -> Namespace {
        if prefix == "xml" {
            return Namespace::Xml;
        }
        self.scopes
            .iter()
            .rev()
            .find(|binding| binding.prefix == prefix)
            .map_or(
                if prefix.is_empty() {
                    Namespace::None
                } else {
                    Namespace::Foreign
                },
                |binding| binding.namespace,
            )
    }

    /// `raw` with its entity and character references replaced.
    fn unescape(&self, raw: &str) -> Result<String, ImageError> {
        if !raw.contains('&') {
            return Ok(raw.to_owned());
        }
        let mut out = String::with_capacity(raw.len());
        let mut rest = raw;
        while let Some(amp) = rest.find('&') {
            out.push_str(&rest[..amp]);
            rest = &rest[amp + 1..];
            let Some(semicolon) = rest.find(';') else {
                return Err(malformed("an '&' that starts no reference"));
            };
            let name = &rest[..semicolon];
            rest = &rest[semicolon + 1..];
            let c = match name {
                "lt" => '<',
                "gt" => '>',
                "amp" => '&',
                "quot" => '"',
                "apos" => '\'',
                _ => {
                    let code = if let Some(hex) = name.strip_prefix("#x") {
                        u32::from_str_radix(hex, 16).ok()
                    } else if let Some(decimal) = name.strip_prefix('#') {
                        decimal.parse::<u32>().ok()
                    } else if self.dtd_entities.iter().any(|entity| entity == name) {
                        return Err(ImageError::Unsupported(format!(
                            "SVG XML: the entity &{name}; a DTD declares"
                        )));
                    } else {
                        return Err(malformed(format_args!("the undeclared entity &{name};")));
                    };
                    code.and_then(char::from_u32)
                        .filter(|&c| c != '\0')
                        .ok_or_else(|| malformed(format_args!("the reference &{name};")))?
                }
            };
            out.push(c);
        }
        out.push_str(rest);
        Ok(out)
    }
}

/// Refuses an XML declaration naming an encoding other than UTF-8 for a file
/// that is not plain ASCII, where the two would read differently.
fn check_declaration(body: &str, text: &str) -> Result<(), ImageError> {
    let Some(at) = body.find("encoding") else {
        return Ok(());
    };
    let value: String = body[at + "encoding".len()..]
        .trim_start_matches([' ', '=', '"', '\''])
        .chars()
        .take_while(|c| !matches!(c, '"' | '\'' | ' '))
        .collect();
    let utf8 = ["utf-8", "utf8", "us-ascii", "ascii"]
        .iter()
        .any(|name| value.eq_ignore_ascii_case(name));
    if utf8 || text.is_ascii() {
        Ok(())
    } else {
        Err(ImageError::Unsupported(format!(
            "SVG XML in the {value} encoding"
        )))
    }
}

fn split_name(qualified: &str) -> (Option<&str>, &str) {
    match qualified.split_once(':') {
        Some((prefix, local)) => (Some(prefix), local),
        None => (None, qualified),
    }
}

/// XML's `NameStartChar` and `NameChar`, close enough: letters, digits and
/// the punctuation names may hold, and anything past ASCII.
fn is_name_char(c: char, first: bool) -> bool {
    c.is_ascii_alphabetic()
        || c == '_'
        || c == ':'
        || !c.is_ascii()
        || (!first && (c.is_ascii_digit() || c == '-' || c == '.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(document: &Document) -> Vec<&str> {
        document
            .elements
            .iter()
            .map(|element| element.name.as_str())
            .collect()
    }

    /// **Elements, attributes, text and CDATA come out as written**, with
    /// comments, processing instructions and the DOCTYPE skipped.
    #[test]
    fn a_document_parses_into_its_elements() {
        let document = parse(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN" "http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd">
<!-- a comment -->
<svg xmlns="http://www.w3.org/2000/svg" width='16'>
  <style><![CDATA[ .a { fill: red } ]]></style>
  <g id="x"><?pi ignored?><rect x="1"/></g>
</svg>"#,
        )
        .expect("parses");
        assert_eq!(names(&document), ["svg", "style", "g", "rect"]);
        let root = document.root();
        assert!(root.is("svg"));
        assert_eq!(root.attribute(Namespace::None, "width"), Some("16"));
        assert_eq!(root.children, [1, 2]);
        assert_eq!(document.elements[1].text, " .a { fill: red } ");
        assert_eq!(document.elements[2].children, [3]);
    }

    /// **The predefined entities and numeric references are replaced; a
    /// whitespace character in an attribute becomes a space.**
    #[test]
    fn references_are_replaced() {
        let document = parse(
            b"<svg xmlns='http://www.w3.org/2000/svg' a='&lt;&gt;&amp;&quot;&apos;&#65;&#x42;' b='x\n\ty'>&#x263A;</svg>",
        )
        .expect("parses");
        let root = document.root();
        assert_eq!(root.attribute(Namespace::None, "a"), Some("<>&\"'AB"));
        assert_eq!(root.attribute(Namespace::None, "b"), Some("x  y"));
        assert_eq!(root.text, "\u{263A}");
    }

    /// **Namespaces resolve through their declarations**, whatever the
    /// prefix, and an element outside SVG's namespace is marked foreign.
    #[test]
    fn namespaces_resolve_through_declarations() {
        let document = parse(
            br##"<s:svg xmlns:s="http://www.w3.org/2000/svg" xmlns:l="http://www.w3.org/1999/xlink" xmlns:ink="http://www.inkscape.org/namespaces/inkscape">
  <s:use l:href="#a" ink:label="x" xml:space="preserve"/>
  <ink:grid/>
  <other xmlns="urn:other"><s:rect/></other>
  <nobody:declared/>
</s:svg>"##,
        )
        .expect("parses");
        let [svg, use_, grid, other, rect, undeclared] = &document.elements[..] else {
            panic!("{:?}", names(&document));
        };
        assert!(svg.is("svg") && use_.is("use") && rect.is("rect"));
        assert_eq!(use_.attribute(Namespace::Xlink, "href"), Some("#a"));
        assert_eq!(use_.attribute(Namespace::Foreign, "label"), Some("x"));
        assert_eq!(use_.attribute(Namespace::Xml, "space"), Some("preserve"));
        assert_eq!(grid.namespace, Namespace::Foreign);
        assert_eq!(other.namespace, Namespace::Foreign);
        assert_eq!(undeclared.namespace, Namespace::Foreign);
    }

    /// **A DTD's entities are never expanded**: the billion-laughs bomb is
    /// refused at its first reference, without growing anything.
    #[test]
    fn dtd_entities_are_refused_not_expanded() {
        let bomb = br#"<?xml version="1.0"?>
<!DOCTYPE svg [
  <!ENTITY lol "lol">
  <!ENTITY lol2 "&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;">
  <!ENTITY lol3 "&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;">
  <!-- ]> a comment that looks like the end -->
]>
<svg xmlns="http://www.w3.org/2000/svg"><title>&lol3;</title></svg>"#;
        let error = parse(bomb).expect_err("refused");
        assert!(
            matches!(&error, ImageError::Unsupported(why) if why.contains("&lol3;")),
            "{error:?}"
        );
        // An external entity is no different: declared, so refused, and never
        // read.
        let external = br#"<!DOCTYPE svg [<!ENTITY x SYSTEM "file:///etc/passwd">]>
<svg xmlns="http://www.w3.org/2000/svg">&x;</svg>"#;
        assert!(matches!(parse(external), Err(ImageError::Unsupported(_))));
        // And one nothing declared is simply malformed.
        assert!(matches!(
            parse(b"<svg xmlns='http://www.w3.org/2000/svg'>&nbsp;</svg>"),
            Err(ImageError::Malformed(_))
        ));
    }

    /// **Nesting past [`MAX_DEPTH`] and elements past [`MAX_ELEMENTS`] are
    /// refused**, and a tree just inside both is not.
    #[test]
    fn depth_and_element_count_are_capped() {
        let nested = |depth: usize| {
            let mut text = String::from("<svg xmlns='http://www.w3.org/2000/svg'>");
            text.push_str(&"<g>".repeat(depth - 1));
            text.push_str(&"</g>".repeat(depth - 1));
            text.push_str("</svg>");
            text
        };
        assert!(parse(nested(MAX_DEPTH).as_bytes()).is_ok());
        assert!(matches!(
            parse(nested(MAX_DEPTH + 1).as_bytes()),
            Err(ImageError::Unsupported(why)) if why.contains("nested")
        ));

        let wide = |count: usize| {
            let mut text = String::from("<svg xmlns='http://www.w3.org/2000/svg'>");
            text.push_str(&"<g/>".repeat(count - 1));
            text.push_str("</svg>");
            text
        };
        assert!(parse(wide(MAX_ELEMENTS).as_bytes()).is_ok());
        assert!(matches!(
            parse(wide(MAX_ELEMENTS + 1).as_bytes()),
            Err(ImageError::Unsupported(why)) if why.contains("elements")
        ));
    }

    /// **What is not well-formed is malformed, not guessed at.**
    #[test]
    fn ill_formed_documents_are_refused() {
        for bad in [
            &b""[..],
            b"<svg>",
            b"<svg></g>",
            b"<svg a=1/>",
            b"<svg a='1' a='2'/>",
            b"<svg/><svg/>",
            b"text<svg/>",
            b"<svg a='<'/>",
            b"<svg>&#0;</svg>",
            b"<svg>&amp</svg>",
            b"<svg><!-- never closed </svg>",
            b"\xff\xfe<svg/>",
        ] {
            assert!(
                matches!(parse(bad), Err(ImageError::Malformed(_))),
                "{:?}",
                String::from_utf8_lossy(bad)
            );
        }
        assert!(matches!(
            parse(b"\x1f\x8b\x08\x00"),
            Err(ImageError::Unsupported(_))
        ));
        assert!(matches!(
            parse("<?xml version='1.0' encoding='ISO-8859-1'?><svg>\u{e9}</svg>".as_bytes()),
            Err(ImageError::Unsupported(_))
        ));
    }
}
