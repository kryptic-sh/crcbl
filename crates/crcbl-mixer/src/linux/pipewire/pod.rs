//! SPA POD, the value encoding every PipeWire message body is written in.
//!
//! A POD is a 32-bit body size, a 32-bit type, then the body, padded to a
//! multiple of eight bytes. A struct's body is a sequence of PODs; an object's
//! is its object type and param id followed by `(key, flags, POD)` properties;
//! an array's is one child header followed by that many packed child bodies.
//! The layout is `spa/pod/pod.h`'s, and the type numbers are
//! `spa/utils/type.h`'s, which [`Type`] spells out.
//!
//! Only the types the mixer reads or writes are decoded into their own
//! [`Value`] arm; anything else is kept as [`Value::Other`] with its type, so a
//! message carrying one still parses.

use std::fmt;

/// `spa/utils/type.h`'s basic types, by number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub(crate) enum Type {
    None = 1,
    Bool = 2,
    Id = 3,
    Int = 4,
    Long = 5,
    Float = 6,
    Double = 7,
    String = 8,
    Bytes = 9,
    Array = 13,
    Struct = 14,
    Object = 15,
    Choice = 19,
}

/// A decoded POD.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Value {
    None,
    Bool(bool),
    Id(u32),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    String(String),
    Bytes(Vec<u8>),
    /// An array's values, each decoded as its child type.
    Array(Vec<Value>),
    Struct(Vec<Value>),
    Object(Object),
    /// A choice's kind (`SPA_CHOICE_*`) and its values, the first of which is
    /// the default.
    Choice(u32, Vec<Value>),
    /// A type this decoder does not open, by number.
    Other(u32),
}

/// An object POD: its object type, its param id, and its properties in order.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Object {
    pub(crate) kind: u32,
    pub(crate) id: u32,
    pub(crate) props: Vec<Property>,
}

/// One property of an [`Object`].
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Property {
    pub(crate) key: u32,
    pub(crate) flags: u32,
    pub(crate) value: Value,
}

impl Object {
    /// The value of property `key`, if the object has it.
    pub(crate) fn get(&self, key: u32) -> Option<&Value> {
        self.props
            .iter()
            .find(|property| property.key == key)
            .map(|property| &property.value)
    }
}

impl Value {
    pub(crate) fn as_int(&self) -> Option<i32> {
        match self {
            Self::Int(value) => Some(*value),
            _ => None,
        }
    }

    pub(crate) fn as_id(&self) -> Option<u32> {
        match self {
            Self::Id(value) => Some(*value),
            _ => None,
        }
    }

    pub(crate) fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    /// The string, or `None` for a null string, which PipeWire writes as
    /// [`Value::None`].
    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    pub(crate) fn as_object(&self) -> Option<&Object> {
        match self {
            Self::Object(object) => Some(object),
            _ => None,
        }
    }

    pub(crate) fn as_struct(&self) -> Option<&[Value]> {
        match self {
            Self::Struct(fields) => Some(fields),
            _ => None,
        }
    }

    /// Every element of an array of floats, as a channel-volume array is.
    pub(crate) fn as_floats(&self) -> Option<Vec<f32>> {
        match self {
            Self::Array(values) => values
                .iter()
                .map(|value| match value {
                    Self::Float(float) => Some(*float),
                    _ => None,
                })
                .collect(),
            _ => None,
        }
    }

    /// The value a choice stands for when read as a setting: its default,
    /// the first value. Anything that is not a choice is itself.
    pub(crate) fn chosen(&self) -> &Self {
        match self {
            Self::Choice(_, values) => values.first().unwrap_or(self),
            _ => self,
        }
    }
}

/// Why a buffer is not a POD this decoder can read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DecodeError(pub(crate) &'static str);

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "malformed POD: {}", self.0)
    }
}

/// The size a body of `size` bytes takes once padded.
fn padded(size: usize) -> usize {
    size.div_ceil(8) * 8
}

fn word(bytes: &[u8], at: usize) -> Result<u32, DecodeError> {
    bytes
        .get(at..at + 4)
        .map(|word| u32::from_ne_bytes(word.try_into().expect("four bytes")))
        .ok_or(DecodeError("truncated"))
}

/// Decodes the POD at the start of `bytes`, and returns it with the number of
/// bytes it took, padding included.
pub(crate) fn decode(bytes: &[u8]) -> Result<(Value, usize), DecodeError> {
    let size = word(bytes, 0)? as usize;
    let kind = word(bytes, 4)?;
    let body = bytes
        .get(8..8 + size)
        .ok_or(DecodeError("a body longer than its buffer"))?;
    Ok((body_of(kind, size, body)?, 8 + padded(size)))
}

/// Decodes a body of type `kind`.
fn body_of(kind: u32, size: usize, body: &[u8]) -> Result<Value, DecodeError> {
    let fixed = |want: usize| {
        if size < want {
            Err(DecodeError("a body shorter than its type"))
        } else {
            Ok(())
        }
    };
    Ok(match kind {
        k if k == Type::None as u32 => Value::None,
        k if k == Type::Bool as u32 => {
            fixed(4)?;
            Value::Bool(word(body, 0)? != 0)
        }
        k if k == Type::Id as u32 => {
            fixed(4)?;
            Value::Id(word(body, 0)?)
        }
        k if k == Type::Int as u32 => {
            fixed(4)?;
            Value::Int(word(body, 0)?.cast_signed())
        }
        k if k == Type::Long as u32 => {
            fixed(8)?;
            Value::Long(i64::from_ne_bytes(
                body[..8].try_into().expect("eight bytes"),
            ))
        }
        k if k == Type::Float as u32 => {
            fixed(4)?;
            Value::Float(f32::from_bits(word(body, 0)?))
        }
        k if k == Type::Double as u32 => {
            fixed(8)?;
            Value::Double(f64::from_ne_bytes(
                body[..8].try_into().expect("eight bytes"),
            ))
        }
        k if k == Type::String as u32 => {
            let text = body.split(|byte| *byte == 0).next().unwrap_or_default();
            Value::String(String::from_utf8_lossy(text).into_owned())
        }
        k if k == Type::Bytes as u32 => Value::Bytes(body.to_vec()),
        k if k == Type::Array as u32 => {
            fixed(8)?;
            let child_size = word(body, 0)? as usize;
            let child_type = word(body, 4)?;
            let values = &body[8..];
            if child_size == 0 {
                Value::Array(Vec::new())
            } else {
                Value::Array(
                    values
                        .chunks_exact(child_size)
                        .map(|chunk| body_of(child_type, child_size, chunk))
                        .collect::<Result<_, _>>()?,
                )
            }
        }
        k if k == Type::Struct as u32 => {
            let mut fields = Vec::new();
            let mut at = 0;
            while at < body.len() {
                let (field, used) = decode(&body[at..])?;
                fields.push(field);
                at += used;
            }
            Value::Struct(fields)
        }
        k if k == Type::Object as u32 => {
            fixed(8)?;
            let mut object = Object {
                kind: word(body, 0)?,
                id: word(body, 4)?,
                props: Vec::new(),
            };
            let mut at = 8;
            while at < body.len() {
                let key = word(body, at)?;
                let flags = word(body, at + 4)?;
                let (value, used) = decode(&body[at + 8..])?;
                object.props.push(Property { key, flags, value });
                at += 8 + used;
            }
            Value::Object(object)
        }
        k if k == Type::Choice as u32 => {
            fixed(16)?;
            let choice = word(body, 0)?;
            let child_size = word(body, 8)? as usize;
            let child_type = word(body, 12)?;
            let values = if child_size == 0 {
                Vec::new()
            } else {
                body[16..]
                    .chunks_exact(child_size)
                    .map(|chunk| body_of(child_type, child_size, chunk))
                    .collect::<Result<_, _>>()?
            };
            Value::Choice(choice, values)
        }
        other => Value::Other(other),
    })
}

/// Writes PODs into a buffer, each padded as the format requires.
#[derive(Default)]
pub(crate) struct Builder {
    bytes: Vec<u8>,
}

impl Builder {
    pub(crate) fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    fn header(&mut self, size: usize, kind: Type) {
        self.bytes.extend_from_slice(
            &u32::try_from(size)
                .expect("a POD under 4 GiB")
                .to_ne_bytes(),
        );
        self.bytes.extend_from_slice(&(kind as u32).to_ne_bytes());
    }

    fn pad(&mut self) {
        let padded = padded(self.bytes.len());
        self.bytes.resize(padded, 0);
    }

    pub(crate) fn none(&mut self) -> &mut Self {
        self.header(0, Type::None);
        self
    }

    pub(crate) fn bool(&mut self, value: bool) -> &mut Self {
        self.word_pod(Type::Bool, u32::from(value))
    }

    pub(crate) fn id(&mut self, value: u32) -> &mut Self {
        self.word_pod(Type::Id, value)
    }

    pub(crate) fn int(&mut self, value: i32) -> &mut Self {
        self.word_pod(Type::Int, value.cast_unsigned())
    }

    fn word_pod(&mut self, kind: Type, value: u32) -> &mut Self {
        self.header(4, kind);
        self.bytes.extend_from_slice(&value.to_ne_bytes());
        self.pad();
        self
    }

    /// A string, or [`Value::None`] for `None`, which is how PipeWire writes a
    /// null string.
    pub(crate) fn string(&mut self, value: Option<&str>) -> &mut Self {
        let Some(value) = value else {
            return self.none();
        };
        self.header(value.len() + 1, Type::String);
        self.bytes.extend_from_slice(value.as_bytes());
        self.bytes.push(0);
        self.pad();
        self
    }

    /// An array of ids.
    pub(crate) fn id_array(&mut self, values: &[u32]) -> &mut Self {
        self.header(8 + 4 * values.len(), Type::Array);
        self.bytes.extend_from_slice(&4u32.to_ne_bytes());
        self.bytes
            .extend_from_slice(&(Type::Id as u32).to_ne_bytes());
        for value in values {
            self.bytes.extend_from_slice(&value.to_ne_bytes());
        }
        self.pad();
        self
    }

    /// An array of floats.
    pub(crate) fn float_array(&mut self, values: &[f32]) -> &mut Self {
        self.header(8 + 4 * values.len(), Type::Array);
        self.bytes.extend_from_slice(&4u32.to_ne_bytes());
        self.bytes
            .extend_from_slice(&(Type::Float as u32).to_ne_bytes());
        for value in values {
            self.bytes.extend_from_slice(&value.to_bits().to_ne_bytes());
        }
        self.pad();
        self
    }

    /// A struct whose fields `fields` writes.
    pub(crate) fn structure(&mut self, fields: impl FnOnce(&mut Self)) -> &mut Self {
        self.framed(Type::Struct, |builder| fields(builder))
    }

    /// An object of type `kind` and param `id`, whose properties `props` writes
    /// through [`Builder::property`].
    pub(crate) fn object(
        &mut self,
        kind: u32,
        id: u32,
        props: impl FnOnce(&mut Self),
    ) -> &mut Self {
        self.framed(Type::Object, |builder| {
            builder.bytes.extend_from_slice(&kind.to_ne_bytes());
            builder.bytes.extend_from_slice(&id.to_ne_bytes());
            props(builder);
        })
    }

    /// A property header; the value is the POD written next.
    pub(crate) fn property(&mut self, key: u32) -> &mut Self {
        self.bytes.extend_from_slice(&key.to_ne_bytes());
        self.bytes.extend_from_slice(&0u32.to_ne_bytes());
        self
    }

    /// A container whose size is known once `body` has written it.
    fn framed(&mut self, kind: Type, body: impl FnOnce(&mut Self)) -> &mut Self {
        let start = self.bytes.len();
        self.header(0, kind);
        body(self);
        let size = self.bytes.len() - start - 8;
        self.bytes[start..start + 4].copy_from_slice(
            &u32::try_from(size)
                .expect("a POD under 4 GiB")
                .to_ne_bytes(),
        );
        self.pad();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(bytes: &[u8]) -> Vec<u32> {
        bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|word| u32::from_ne_bytes(*word))
            .collect()
    }

    /// The bytes `spa_pod_builder_add_struct(b, SPA_POD_Int(4))` writes:
    /// `core.hello`'s body. Pinned word by word, because both ends being this
    /// file would agree on any mistake.
    #[test]
    fn a_struct_of_one_int_is_the_bytes_spa_writes() {
        let mut builder = Builder::default();
        builder.structure(|b| {
            b.int(4);
        });
        assert_eq!(
            words(&builder.into_bytes()),
            [16, Type::Struct as u32, 4, Type::Int as u32, 4, 0]
        );
    }

    /// A string carries its NUL and pads to eight; a null one is `None`.
    #[test]
    fn a_string_is_nul_terminated_and_padded() {
        let mut builder = Builder::default();
        builder.string(Some("Node")).string(None);
        let bytes = builder.into_bytes();
        assert_eq!(words(&bytes[..8]), [5, Type::String as u32]);
        assert_eq!(&bytes[8..13], b"Node\0");
        assert_eq!(bytes.len(), 16 + 8, "padded to 16, then an empty None");
        assert_eq!(words(&bytes[16..]), [0, Type::None as u32]);
    }

    /// Every arm the mixer writes decodes back to itself, nested.
    #[test]
    fn what_the_builder_writes_the_decoder_reads() {
        let mut builder = Builder::default();
        builder.structure(|b| {
            b.int(-3)
                .id(13)
                .bool(true)
                .string(Some("alsa_output.usb"))
                .string(None)
                .id_array(&[2, 13])
                .object(262_146, 2, |b| {
                    b.property(65_540).bool(false);
                    b.property(65_544).float_array(&[0.25, 0.5]);
                });
        });
        let bytes = builder.into_bytes();
        let (value, used) = decode(&bytes).unwrap();
        assert_eq!(used, bytes.len());
        let fields = value.as_struct().unwrap();
        assert_eq!(fields[0], Value::Int(-3));
        assert_eq!(fields[1], Value::Id(13));
        assert_eq!(fields[2], Value::Bool(true));
        assert_eq!(fields[3].as_str(), Some("alsa_output.usb"));
        assert_eq!(fields[4], Value::None);
        assert_eq!(fields[5], Value::Array(vec![Value::Id(2), Value::Id(13)]));
        let object = fields[6].as_object().unwrap();
        assert_eq!((object.kind, object.id), (262_146, 2));
        assert_eq!(object.get(65_540), Some(&Value::Bool(false)));
        assert_eq!(
            object.get(65_544).unwrap().as_floats(),
            Some(vec![0.25, 0.5])
        );
    }

    /// A choice reads as its default, and a buffer cut short is an error, not
    /// a panic.
    #[test]
    fn a_choice_reads_as_its_default_and_a_short_buffer_is_refused() {
        // SPA_CHOICE_Range of floats: default 0.5, min 0, max 1.
        let mut bytes = Vec::new();
        for word in [16u32 + 12, Type::Choice as u32, 1, 0, 4, Type::Float as u32] {
            bytes.extend_from_slice(&word.to_ne_bytes());
        }
        for float in [0.5f32, 0.0, 1.0] {
            bytes.extend_from_slice(&float.to_bits().to_ne_bytes());
        }
        bytes.resize(padded(bytes.len()), 0);
        let (value, _) = decode(&bytes).unwrap();
        assert_eq!(value.chosen(), &Value::Float(0.5));
        assert!(decode(&bytes[..10]).is_err());
    }
}
