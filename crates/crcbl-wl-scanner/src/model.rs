//! The protocol model: what a Wayland XML file says, in Rust types.
//!
//! Everything the wire format needs and nothing it does not. Descriptions,
//! summaries, `deprecated-since` and enum documentation are parsed past and
//! dropped — they cannot affect a single byte on the socket, and carrying them
//! would only widen the surface that has to stay correct.
//!
//! Enums *are* kept, because `docs/plan/15-windowing.md`'s backends read
//! `xdg_toplevel.state` and `wl_output.transform` by name, and a hand-copied
//! integer that drifts from the XML is exactly the class of bug this generator
//! exists to remove.

use core::fmt;

use crate::xml::{Node, Reader, XmlError};

/// One wire argument type.
///
/// The discriminant characters are the ones libwayland's `wl_message.signature`
/// uses; see [`ArgType::signature_char`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArgType {
    /// `int` — signed 32-bit.
    Int,
    /// `uint` — unsigned 32-bit.
    Uint,
    /// `fixed` — 24.8 signed fixed point, carried as `i32`.
    Fixed,
    /// `string` — NUL-terminated UTF-8.
    String,
    /// `object` — an existing proxy.
    Object,
    /// `new_id` — an object this message creates.
    NewId,
    /// `array` — a length-prefixed blob.
    Array,
    /// `fd` — a file descriptor passed out of band.
    Fd,
}

impl ArgType {
    /// Parses the `type=` attribute.
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "int" => Self::Int,
            "uint" => Self::Uint,
            "fixed" => Self::Fixed,
            "string" => Self::String,
            "object" => Self::Object,
            "new_id" => Self::NewId,
            "array" => Self::Array,
            "fd" => Self::Fd,
            _ => return None,
        })
    }

    /// The character this type contributes to a `wl_message` signature.
    #[must_use]
    pub const fn signature_char(self) -> char {
        match self {
            Self::Int => 'i',
            Self::Uint => 'u',
            Self::Fixed => 'f',
            Self::String => 's',
            Self::Object => 'o',
            Self::NewId => 'n',
            Self::Array => 'a',
            Self::Fd => 'h',
        }
    }
}

/// One argument of a request or event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Arg {
    /// Argument name, as written in the XML.
    pub name: String,
    /// Wire type.
    pub ty: ArgType,
    /// Target interface for [`ArgType::Object`] and [`ArgType::NewId`].
    ///
    /// `None` on a `new_id` means the interface is chosen at runtime — the
    /// `wl_registry.bind` shape, which expands to three wire arguments. See
    /// [`Message::signature`].
    pub interface: Option<String>,
    /// Whether `null` is a legal value (`allow-null="true"`).
    pub allow_null: bool,
    /// The `enum=` annotation, if any — `"wl_output.transform"` style.
    pub enumeration: Option<String>,
}

/// A request or an event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    /// Message name, as written in the XML.
    pub name: String,
    /// Interface version this message appeared in. `1` when unstated.
    pub since: u32,
    /// Whether sending this destroys the object (`type="destructor"`).
    pub destructor: bool,
    /// Arguments in wire order.
    pub args: Vec<Arg>,
}

impl Message {
    /// The `wl_message.signature` string libwayland expects.
    ///
    /// Three rules, all of which produce silent wire corruption when broken —
    /// which is why [`crate`]'s tests compare this against `wayland-scanner`'s
    /// own output for real interfaces:
    ///
    /// 1. A `since` greater than 1 is a decimal prefix (`"2i"`, `"7"`).
    /// 2. A nullable argument is prefixed with `?`.
    /// 3. A `new_id` with no declared interface expands to **three** wire
    ///    arguments — interface name, version, id — so it contributes `"sun"`.
    ///
    /// ```
    /// # use crcbl_wl_scanner::parse_protocol;
    /// let protocol = parse_protocol(
    ///     r#"<protocol name="p"><interface name="wl_registry" version="1">
    ///          <request name="bind">
    ///            <arg name="name" type="uint"/>
    ///            <arg name="id" type="new_id"/>
    ///          </request>
    ///        </interface></protocol>"#,
    /// )?;
    /// assert_eq!(protocol.interfaces[0].requests[0].signature(), "usun");
    /// # Ok::<(), crcbl_wl_scanner::ScanError>(())
    /// ```
    #[must_use]
    pub fn signature(&self) -> String {
        let mut signature = String::new();
        if self.since > 1 {
            signature.push_str(&self.since.to_string());
        }
        for arg in &self.args {
            if arg.allow_null {
                signature.push('?');
            }
            if arg.ty == ArgType::NewId && arg.interface.is_none() {
                signature.push_str("sun");
            } else {
                signature.push(arg.ty.signature_char());
            }
        }
        signature
    }

    /// The interface each wire argument slot refers to, `None` for the slots
    /// that name no object.
    ///
    /// This is the `wl_message.types` row. Its length matches
    /// [`signature`](Self::signature) minus the `since` prefix and the `?`
    /// markers, which is *not* `args.len()` whenever an untyped `new_id` is
    /// present — the three-slot expansion is the single easiest thing to get
    /// wrong here, and getting it wrong shifts every later interface pointer by
    /// two.
    #[must_use]
    pub fn type_slots(&self) -> Vec<Option<&str>> {
        let mut slots = Vec::new();
        for arg in &self.args {
            match (arg.ty, arg.interface.as_deref()) {
                (ArgType::NewId, None) => slots.extend([None, None, None]),
                (ArgType::Object | ArgType::NewId, interface) => slots.push(interface),
                _ => slots.push(None),
            }
        }
        slots
    }

    /// Whether every slot in [`type_slots`](Self::type_slots) is `None`.
    ///
    /// `wayland-scanner` points every such message at the shared leading run of
    /// nulls instead of giving it a row of its own; reproducing that is what
    /// makes the generated table byte-comparable with the reference one.
    #[must_use]
    pub fn all_null(&self) -> bool {
        self.type_slots().iter().all(Option::is_none)
    }
}

/// One `<enum>` — kept because backends read these by name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Enumeration {
    /// Enum name.
    pub name: String,
    /// Whether the values are flags rather than alternatives.
    pub bitfield: bool,
    /// Entries in document order.
    pub entries: Vec<(String, u32)>,
}

/// One `<interface>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Interface {
    /// Wire name, e.g. `"wl_surface"`.
    pub name: String,
    /// Highest version this file describes.
    pub version: u32,
    /// Requests in opcode order.
    pub requests: Vec<Message>,
    /// Events in opcode order.
    pub events: Vec<Message>,
    /// Enumerations declared on this interface.
    pub enums: Vec<Enumeration>,
}

/// One protocol XML file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Protocol {
    /// The `name=` attribute — `"wayland"`, `"xdg_shell"`.
    pub name: String,
    /// Interfaces in document order. Opcode assignment depends on this order,
    /// so it is never sorted.
    pub interfaces: Vec<Interface>,
}

/// A protocol file this generator refuses to guess about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScanError {
    /// The document is not well-formed.
    Xml(XmlError),
    /// A required attribute was missing, or a value made no sense.
    Schema(String),
}

impl fmt::Display for ScanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Xml(error) => write!(f, "{error}"),
            Self::Schema(message) => write!(f, "unsupported protocol file: {message}"),
        }
    }
}

impl std::error::Error for ScanError {}

impl From<XmlError> for ScanError {
    fn from(error: XmlError) -> Self {
        Self::Xml(error)
    }
}

fn attr<'a>(attrs: &'a [(&str, String)], key: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, value)| value.as_str())
}

fn required<'a>(attrs: &'a [(&str, String)], key: &str, what: &str) -> Result<&'a str, ScanError> {
    attr(attrs, key).ok_or_else(|| ScanError::Schema(format!("<{what}> has no {key}= attribute")))
}

fn number(value: &str, what: &str) -> Result<u32, ScanError> {
    // Enum entries are decimal or hex (`0x1`); versions are always decimal.
    let parsed = match value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        Some(hex) => u32::from_str_radix(hex, 16),
        None => value.parse(),
    };
    parsed.map_err(|_| ScanError::Schema(format!("{what} is not a number: {value:?}")))
}

/// Parses one protocol XML document.
///
/// # Errors
///
/// [`ScanError::Xml`] if the document is malformed, [`ScanError::Schema`] if it
/// is well-formed but says something this generator does not understand — an
/// unknown argument type, a missing `name=`. Both are hard failures: a protocol
/// file we half-understand generates a marshaller that corrupts the wire.
pub fn parse_protocol(source: &str) -> Result<Protocol, ScanError> {
    let mut reader = Reader::new(source);
    let mut protocol: Option<Protocol> = None;
    // The element stack, so `<arg>` knows whether it belongs to a request or an
    // event, and `<entry>` to which enum.
    let mut interface: Option<Interface> = None;
    let mut message: Option<(bool, Message)> = None;
    let mut enumeration: Option<Enumeration> = None;

    while let Some(node) = reader.next_node()? {
        match node {
            Node::Text { .. } => {}
            Node::Start { name, attrs, empty } => match name {
                "protocol" => {
                    protocol = Some(Protocol {
                        name: required(&attrs, "name", "protocol")?.to_string(),
                        interfaces: Vec::new(),
                    });
                }
                "interface" => {
                    interface = Some(Interface {
                        name: required(&attrs, "name", "interface")?.to_string(),
                        version: number(required(&attrs, "version", "interface")?, "version")?,
                        requests: Vec::new(),
                        events: Vec::new(),
                        enums: Vec::new(),
                    });
                }
                "request" | "event" => {
                    let parsed = Message {
                        name: required(&attrs, "name", name)?.to_string(),
                        since: match attr(&attrs, "since") {
                            Some(value) => number(value, "since")?,
                            None => 1,
                        },
                        destructor: attr(&attrs, "type") == Some("destructor"),
                        args: Vec::new(),
                    };
                    message = Some((name == "request", parsed));
                    if empty {
                        finish_message(&mut interface, &mut message)?;
                    }
                }
                "arg" => {
                    let Some((_, message)) = message.as_mut() else {
                        return Err(ScanError::Schema(
                            "<arg> outside a request or event".to_string(),
                        ));
                    };
                    let ty = required(&attrs, "type", "arg")?;
                    let ty = ArgType::parse(ty)
                        .ok_or_else(|| ScanError::Schema(format!("unknown arg type {ty:?}")))?;
                    message.args.push(Arg {
                        name: required(&attrs, "name", "arg")?.to_string(),
                        ty,
                        interface: attr(&attrs, "interface").map(ToString::to_string),
                        allow_null: attr(&attrs, "allow-null") == Some("true"),
                        enumeration: attr(&attrs, "enum").map(ToString::to_string),
                    });
                }
                "enum" => {
                    enumeration = Some(Enumeration {
                        name: required(&attrs, "name", "enum")?.to_string(),
                        bitfield: attr(&attrs, "bitfield") == Some("true"),
                        entries: Vec::new(),
                    });
                }
                "entry" => {
                    let Some(enumeration) = enumeration.as_mut() else {
                        return Err(ScanError::Schema("<entry> outside an enum".to_string()));
                    };
                    enumeration.entries.push((
                        required(&attrs, "name", "entry")?.to_string(),
                        number(required(&attrs, "value", "entry")?, "entry value")?,
                    ));
                }
                // `description`, `copyright` and anything a future protocol
                // revision adds are wire-irrelevant and skipped rather than
                // rejected: the generator must keep working when upstream adds
                // documentation markup.
                _ => {}
            },
            Node::End { name } => match name {
                "interface" => {
                    let Some(finished) = interface.take() else {
                        return Err(ScanError::Schema(
                            "</interface> with no opening".to_string(),
                        ));
                    };
                    let Some(protocol) = protocol.as_mut() else {
                        return Err(ScanError::Schema(
                            "<interface> outside <protocol>".to_string(),
                        ));
                    };
                    protocol.interfaces.push(finished);
                }
                "request" | "event" => finish_message(&mut interface, &mut message)?,
                "enum" => {
                    let Some(finished) = enumeration.take() else {
                        return Err(ScanError::Schema("</enum> with no opening".to_string()));
                    };
                    let Some(interface) = interface.as_mut() else {
                        return Err(ScanError::Schema("<enum> outside <interface>".to_string()));
                    };
                    interface.enums.push(finished);
                }
                _ => {}
            },
        }
    }

    protocol.ok_or_else(|| ScanError::Schema("no <protocol> element".to_string()))
}

fn finish_message(
    interface: &mut Option<Interface>,
    message: &mut Option<(bool, Message)>,
) -> Result<(), ScanError> {
    let Some((is_request, finished)) = message.take() else {
        return Ok(());
    };
    let Some(interface) = interface.as_mut() else {
        return Err(ScanError::Schema(
            "a request or event outside <interface>".to_string(),
        ));
    };
    if is_request {
        interface.requests.push(finished);
    } else {
        interface.events.push(finished);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
        <protocol name="sample">
          <interface name="wl_thing" version="3">
            <description summary="ignored">body</description>
            <request name="destroy" type="destructor"/>
            <request name="attach">
              <arg name="buffer" type="object" interface="wl_buffer" allow-null="true"/>
              <arg name="x" type="int"/>
              <arg name="y" type="int"/>
            </request>
            <request name="set_scale" since="3">
              <arg name="scale" type="int"/>
            </request>
            <event name="enter">
              <arg name="output" type="object" interface="wl_output"/>
            </event>
            <enum name="error" bitfield="false">
              <entry name="bad" value="0"/>
              <entry name="worse" value="0x2"/>
            </enum>
          </interface>
        </protocol>"#;

    #[test]
    fn a_protocol_round_trips_into_the_model() {
        let protocol = parse_protocol(SAMPLE).expect("well-formed");
        assert_eq!(protocol.name, "sample");
        let interface = &protocol.interfaces[0];
        assert_eq!(interface.name, "wl_thing");
        assert_eq!(interface.version, 3);
        assert_eq!(
            interface.requests.len(),
            3,
            "including the empty destructor"
        );
        assert_eq!(interface.events.len(), 1);

        let destroy = &interface.requests[0];
        assert!(destroy.destructor, "type=destructor is load-bearing");
        assert!(destroy.args.is_empty());
        assert_eq!(destroy.signature(), "");

        let attach = &interface.requests[1];
        assert_eq!(attach.signature(), "?oii");
        assert_eq!(
            attach.type_slots(),
            vec![Some("wl_buffer"), None, None],
            "only the object slot names an interface"
        );
        assert!(!attach.all_null());

        assert_eq!(interface.requests[2].signature(), "3i");
        assert!(interface.requests[2].all_null());

        assert_eq!(
            interface.enums[0].entries,
            [("bad".to_string(), 0), ("worse".to_string(), 2)]
        );
    }

    #[test]
    fn an_untyped_new_id_occupies_three_slots() {
        // The `wl_registry.bind` shape. Getting this wrong shifts every later
        // interface pointer in the protocol's type table by two.
        let protocol = parse_protocol(
            r#"<protocol name="p"><interface name="i" version="1">
                 <request name="bind">
                   <arg name="name" type="uint"/>
                   <arg name="id" type="new_id"/>
                 </request>
               </interface></protocol>"#,
        )
        .expect("well-formed");
        let bind = &protocol.interfaces[0].requests[0];
        assert_eq!(bind.signature(), "usun");
        assert_eq!(bind.type_slots().len(), 4);
        assert!(bind.all_null(), "no slot names an interface");
    }

    #[test]
    fn unknown_argument_types_are_refused_rather_than_guessed() {
        let error = parse_protocol(
            r#"<protocol name="p"><interface name="i" version="1">
                 <event name="e"><arg name="a" type="quaternion"/></event>
               </interface></protocol>"#,
        )
        .expect_err("unknown type");
        assert!(error.to_string().contains("quaternion"), "{error}");

        let error = parse_protocol("<protocol><interface/></protocol>").expect_err("no name");
        assert!(error.to_string().contains("name="), "{error}");

        assert!(
            parse_protocol("<nothing/>").is_err(),
            "no <protocol> element"
        );
    }
}
