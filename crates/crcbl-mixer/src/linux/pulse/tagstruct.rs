//! PulseAudio's tagstruct: the encoding every native-protocol packet is in.
//!
//! Each value is a one-byte tag and then its data, big-endian: `t` a
//! NUL-terminated string and `N` a null one, `L` a `u32`, `B` a `u8`, `R`/`r`
//! and `U` 64-bit values, `1`/`0` a boolean, `x` a length and bytes, `a` a
//! sample spec, `m` a channel map, `v` a channel volume, `V` one volume, `P` a
//! property list and `f` a format. The tags and layouts are PulseAudio's
//! `pulsecore/tagstruct.c`, which `module-protocol-pulse`'s `message.c`
//! implements from the other end.

use std::collections::BTreeMap;

/// One tagged value.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Tag {
    Str(Option<String>),
    U32(u32),
    U8(u8),
    U64(u64),
    Bool(bool),
    Bytes(Vec<u8>),
    /// A sample spec: format, channels, rate.
    SampleSpec,
    /// A channel map's channel count.
    ChannelMap(u8),
    /// Each channel's volume, where 65 536 is 100 %.
    Volumes(Vec<u32>),
    Volume(u32),
    Props(BTreeMap<String, String>),
    /// A format: its encoding and properties.
    Format,
}

/// Why a packet is not a tagstruct this reader can read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DecodeError(pub(crate) String);

/// Reads tagged values off a packet, front to back.
pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.at >= self.bytes.len()
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        let end = self
            .at
            .checked_add(n)
            .filter(|end| *end <= self.bytes.len());
        let end = end.ok_or_else(|| DecodeError("a packet shorter than its values".into()))?;
        let taken = &self.bytes[self.at..end];
        self.at = end;
        Ok(taken)
    }

    fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, DecodeError> {
        Ok(u32::from_be_bytes(
            self.take(4)?.try_into().expect("four bytes"),
        ))
    }

    fn u64(&mut self) -> Result<u64, DecodeError> {
        Ok(u64::from_be_bytes(
            self.take(8)?.try_into().expect("eight bytes"),
        ))
    }

    fn cstr(&mut self) -> Result<String, DecodeError> {
        let rest = &self.bytes[self.at..];
        let len = rest
            .iter()
            .position(|byte| *byte == 0)
            .ok_or_else(|| DecodeError("an unterminated string".into()))?;
        let text = String::from_utf8_lossy(&rest[..len]).into_owned();
        self.at += len + 1;
        Ok(text)
    }

    /// The next value.
    pub(crate) fn next(&mut self) -> Result<Tag, DecodeError> {
        let tag = self.u8()?;
        Ok(match tag {
            b't' => Tag::Str(Some(self.cstr()?)),
            b'N' => Tag::Str(None),
            b'L' => Tag::U32(self.u32()?),
            b'B' => Tag::U8(self.u8()?),
            b'R' | b'r' | b'U' => Tag::U64(self.u64()?),
            b'1' => Tag::Bool(true),
            b'0' => Tag::Bool(false),
            b'x' => {
                let len = self.u32()? as usize;
                Tag::Bytes(self.take(len)?.to_vec())
            }
            b'a' => {
                self.take(2 + 4)?;
                Tag::SampleSpec
            }
            b'm' => {
                let channels = self.u8()?;
                self.take(usize::from(channels))?;
                Tag::ChannelMap(channels)
            }
            b'v' => {
                let channels = self.u8()?;
                Tag::Volumes(
                    (0..channels)
                        .map(|_| self.u32())
                        .collect::<Result<_, _>>()?,
                )
            }
            b'V' => Tag::Volume(self.u32()?),
            b'T' => {
                self.take(8)?;
                Tag::U64(0)
            }
            b'P' => Tag::Props(self.props()?),
            b'f' => {
                match self.next()? {
                    Tag::U8(_) => {}
                    other => return Err(DecodeError(format!("a format's encoding was {other:?}"))),
                }
                match self.next()? {
                    Tag::Props(_) => {}
                    other => return Err(DecodeError(format!("a format's props were {other:?}"))),
                }
                Tag::Format
            }
            other => {
                return Err(DecodeError(format!(
                    "an unknown tag {:?}",
                    char::from(other)
                )));
            }
        })
    }

    /// A property list's body: key, length and value triples, ended by a null
    /// string. A value is bytes, a string's NUL included.
    fn props(&mut self) -> Result<BTreeMap<String, String>, DecodeError> {
        let mut props = BTreeMap::new();
        loop {
            let Tag::Str(key) = self.next()? else {
                return Err(DecodeError("a property list's key is not a string".into()));
            };
            let Some(key) = key else {
                return Ok(props);
            };
            let _length = self.u32_tag()?;
            let Tag::Bytes(value) = self.next()? else {
                return Err(DecodeError("a property's value is not bytes".into()));
            };
            let value = value.strip_suffix(&[0]).unwrap_or(&value);
            props.insert(key, String::from_utf8_lossy(value).into_owned());
        }
    }

    // --- typed reads, for a layout that knows what comes next ---------------

    pub(crate) fn u32_tag(&mut self) -> Result<u32, DecodeError> {
        match self.next()? {
            Tag::U32(value) => Ok(value),
            other => Err(DecodeError(format!("expected a u32, got {other:?}"))),
        }
    }

    pub(crate) fn str_tag(&mut self) -> Result<Option<String>, DecodeError> {
        match self.next()? {
            Tag::Str(value) => Ok(value),
            other => Err(DecodeError(format!("expected a string, got {other:?}"))),
        }
    }

    pub(crate) fn bool_tag(&mut self) -> Result<bool, DecodeError> {
        match self.next()? {
            Tag::Bool(value) => Ok(value),
            other => Err(DecodeError(format!("expected a boolean, got {other:?}"))),
        }
    }

    pub(crate) fn u8_tag(&mut self) -> Result<u8, DecodeError> {
        match self.next()? {
            Tag::U8(value) => Ok(value),
            other => Err(DecodeError(format!("expected a u8, got {other:?}"))),
        }
    }

    pub(crate) fn volumes_tag(&mut self) -> Result<Vec<u32>, DecodeError> {
        match self.next()? {
            Tag::Volumes(value) => Ok(value),
            other => Err(DecodeError(format!(
                "expected a channel volume, got {other:?}"
            ))),
        }
    }

    pub(crate) fn props_tag(&mut self) -> Result<BTreeMap<String, String>, DecodeError> {
        match self.next()? {
            Tag::Props(value) => Ok(value),
            other => Err(DecodeError(format!(
                "expected a property list, got {other:?}"
            ))),
        }
    }

    /// Reads one value and ignores it, whatever it is.
    pub(crate) fn skip(&mut self) -> Result<(), DecodeError> {
        self.next().map(drop)
    }
}

/// Writes tagged values.
#[derive(Default)]
pub(crate) struct Writer {
    bytes: Vec<u8>,
}

impl Writer {
    pub(crate) fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    pub(crate) fn u32(&mut self, value: u32) -> &mut Self {
        self.bytes.push(b'L');
        self.bytes.extend_from_slice(&value.to_be_bytes());
        self
    }

    pub(crate) fn str(&mut self, value: Option<&str>) -> &mut Self {
        match value {
            Some(value) => {
                self.bytes.push(b't');
                self.bytes.extend_from_slice(value.as_bytes());
                self.bytes.push(0);
            }
            None => self.bytes.push(b'N'),
        }
        self
    }

    pub(crate) fn bool(&mut self, value: bool) -> &mut Self {
        self.bytes.push(if value { b'1' } else { b'0' });
        self
    }

    pub(crate) fn bytes(&mut self, value: &[u8]) -> &mut Self {
        self.bytes.push(b'x');
        self.bytes
            .extend_from_slice(&u32::try_from(value.len()).expect("small").to_be_bytes());
        self.bytes.extend_from_slice(value);
        self
    }

    /// A channel volume: one volume for each of `channels` channels.
    pub(crate) fn volumes(&mut self, volume: u32, channels: u8) -> &mut Self {
        self.bytes.push(b'v');
        self.bytes.push(channels);
        for _ in 0..channels {
            self.bytes.extend_from_slice(&volume.to_be_bytes());
        }
        self
    }

    pub(crate) fn props(&mut self, props: &[(&str, &str)]) -> &mut Self {
        self.bytes.push(b'P');
        for (key, value) in props {
            self.str(Some(key));
            let mut bytes = value.as_bytes().to_vec();
            bytes.push(0);
            self.u32(u32::try_from(bytes.len()).expect("small"));
            self.bytes(&bytes);
        }
        self.str(None);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bytes `pa_tagstruct` writes for a `u32`, a string, a null string and
    /// a boolean, pinned byte by byte.
    #[test]
    fn values_are_written_as_pulseaudio_tags_them() {
        let mut writer = Writer::default();
        writer.u32(0x0102_0304).str(Some("ab")).str(None).bool(true);
        assert_eq!(
            writer.into_bytes(),
            [b'L', 1, 2, 3, 4, b't', b'a', b'b', 0, b'N', b'1']
        );
    }

    /// What the writer writes the reader reads back, a property list and a
    /// channel volume among it.
    #[test]
    fn what_is_written_reads_back() {
        let mut writer = Writer::default();
        writer
            .u32(7)
            .volumes(32_768, 2)
            .props(&[("application.name", "crcbl")])
            .bool(false);
        let bytes = writer.into_bytes();
        let mut reader = Reader::new(&bytes);
        assert_eq!(reader.u32_tag().unwrap(), 7);
        assert_eq!(reader.volumes_tag().unwrap(), [32_768, 32_768]);
        assert_eq!(
            reader
                .props_tag()
                .unwrap()
                .get("application.name")
                .map(String::as_str),
            Some("crcbl")
        );
        assert!(!reader.bool_tag().unwrap());
        assert!(reader.is_empty());
    }

    /// A cut-off packet and an unknown tag are errors, not panics.
    #[test]
    fn a_short_or_unknown_value_is_refused() {
        assert!(Reader::new(&[b'L', 0, 0]).next().is_err());
        assert!(Reader::new(b"?").next().is_err());
        assert!(Reader::new(b"tunterminated").next().is_err());
    }
}
