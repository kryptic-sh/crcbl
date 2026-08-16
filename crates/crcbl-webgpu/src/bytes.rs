//! The byte primitives both directions are built out of.
//!
//! Two streams cross this seam and they are the *same* format read in opposite
//! directions: the command stream ([`writer`](crate::writer) →
//! [`reader`](crate::reader)) wasm → JS, and the reply stream
//! ([`reply`](crate::reply)) JS → wasm. Little-endian throughout, a tag byte
//! first, a `u32` length prefix before every variable-length field, a cap on
//! every length, and a bounds-checked cursor rather than hand-rolled offset
//! arithmetic.
//!
//! **Shared rather than copied.** A second reader written beside the first is
//! two places for a bound to be wrong, and the two would be *nearly* identical,
//! which is the shape that drifts without anyone noticing. The line is what a
//! field *belongs* to rather than what it is made of: a handle and a
//! [`Features`] word cross in both directions and live here, while
//! [`reader`](crate::reader) and [`writer`](crate::writer) add their
//! descriptor-shaped methods to [`ByteReader`] and [`ByteWriter`] in their own
//! modules, because those belong to the command stream and to nothing else — as
//! [`reply`](crate::reply)'s capability records belong to that direction.

use crcbl_core::Handle;
use crcbl_hal::Features;

use crate::tag;

// ── Error type ────────────────────────────────────────────────────────────────

/// Everything a malformed buffer can be, in either direction.
///
/// Shaped after [`crcbl_net`'s `DecodeError`](https://docs.rs/crcbl-net), which
/// is the house wire style: the variants say where the decode stopped and what
/// it wanted, so a report names a field rather than a buffer.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    /// The buffer does not start with the magic its direction claims —
    /// [`tag::STREAM_MAGIC`] for a command stream, [`tag::REPLY_MAGIC`] for a
    /// reply stream.
    #[error("not a crcbl-webgpu buffer: bad magic")]
    BadMagic,
    /// The buffer's version is not one this build speaks. Reachable in a browser
    /// in a way it is not in a single binary — the Rust and JS halves ship as
    /// separate artifacts and are cached independently.
    #[error("stream format version {found}, expected {expected}")]
    UnsupportedVersion {
        /// Version the buffer declared.
        found: u16,
        /// Version this build writes and reads.
        expected: u16,
    },
    /// A read ran off the end of the buffer.
    #[error("stream too short: need {needed} bytes at offset {offset}, have {remaining}")]
    TooShort {
        /// Bytes the field wanted.
        needed: usize,
        /// Where it started.
        offset: usize,
        /// Bytes actually left.
        remaining: usize,
    },
    /// A tag byte no command in [`tag`] claims, or no reply. Distinguishable
    /// from a malformed known body precisely because the tag comes first.
    #[error("unknown tag: 0x{tag:02x}")]
    UnknownTag {
        /// The byte that was read.
        tag: u8,
    },
    /// A length or element count past the cap for its field.
    #[error("invalid length for {field}: {len}")]
    InvalidLength {
        /// Field that declared it.
        field: &'static str,
        /// The length declared.
        len: u32,
    },
    /// An enum code, bitflags value or presence byte no variant claims.
    #[error("invalid code for {field}: 0x{code:x}")]
    InvalidEnum {
        /// Field that carried it.
        field: &'static str,
        /// The code that was read, widened to the largest a field can carry.
        ///
        /// A `u64` because [`Features`](crcbl_hal::Features) is a 64-bit
        /// bitflags and is refused through `from_bits` rather than truncated —
        /// a narrower field here would have to drop the half of the value that
        /// says which bit was unclaimed.
        code: u64,
    },
    /// A handle field that may not be `None` held zero bits.
    #[error("{field} is a null handle")]
    NullHandle {
        /// Field that carried it.
        field: &'static str,
    },
    /// A length-prefixed string that is not UTF-8.
    #[error("{field} is not valid UTF-8")]
    NotUtf8 {
        /// Field that carried it.
        field: &'static str,
    },
    /// A reply naming a sequence number nothing is waiting on.
    ///
    /// **Only the reply direction produces this**, and it is the one error in
    /// this list that is not about bytes: the buffer decoded, and then answered
    /// a command that was never asked. See
    /// [`StreamChannel::drain_replies`](crate::web::StreamChannel::drain_replies)
    /// for why that is reported rather than dropped.
    #[error("nothing is waiting on a reply for sequence {sequence}")]
    UnexpectedSequence {
        /// The sequence the reply named.
        sequence: u64,
    },
}

// ── ByteReader ────────────────────────────────────────────────────────────────

/// Cursor over a buffer that bounds-checks every read.
///
/// Every decode in this crate goes through this rather than hand-rolling
/// `if cursor + N > buf.len()` per field: that is one more chance to get a bound
/// wrong at each of them, and this seam has more fields than the network one
/// this is copied from.
pub(crate) struct ByteReader<'a> {
    buf: &'a [u8],
    offset: usize,
}

impl core::fmt::Debug for ByteReader<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ByteReader")
            .field("len", &self.buf.len())
            .field("offset", &self.offset)
            .finish()
    }
}

impl<'a> ByteReader<'a> {
    pub(crate) const fn new(buf: &'a [u8]) -> Self {
        Self { buf, offset: 0 }
    }

    pub(crate) const fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.offset)
    }

    pub(crate) const fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    /// The magic and version word every buffer in this crate opens with.
    ///
    /// Both directions have one, and both check it for the same reason: the
    /// Rust and JS halves ship as separate artifacts and are cached
    /// independently, so a decoder meeting a buffer from a different build is
    /// reachable in a browser in a way it is not in a single binary.
    pub(crate) fn read_format_header(
        &mut self,
        magic: &[u8; 8],
        version: u16,
    ) -> Result<(), DecodeError> {
        if self.read_bytes(magic.len())? != magic {
            return Err(DecodeError::BadMagic);
        }
        let found = self.read_u16()?;
        if found != version {
            return Err(DecodeError::UnsupportedVersion {
                found,
                expected: version,
            });
        }
        Ok(())
    }

    pub(crate) fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], DecodeError> {
        if self.remaining() < len {
            return Err(DecodeError::TooShort {
                needed: len,
                offset: self.offset,
                remaining: self.remaining(),
            });
        }
        let slice = &self.buf[self.offset..self.offset + len];
        self.offset += len;
        Ok(slice)
    }

    fn read_array<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        let bytes = self.read_bytes(N)?;
        Ok(bytes
            .try_into()
            .expect("read_bytes yields exactly the length it was asked for"))
    }

    pub(crate) fn read_u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.read_array::<1>()?[0])
    }

    pub(crate) fn read_u16(&mut self) -> Result<u16, DecodeError> {
        Ok(u16::from_le_bytes(self.read_array()?))
    }

    pub(crate) fn read_u32(&mut self) -> Result<u32, DecodeError> {
        Ok(u32::from_le_bytes(self.read_array()?))
    }

    pub(crate) fn read_i32(&mut self) -> Result<i32, DecodeError> {
        Ok(i32::from_le_bytes(self.read_array()?))
    }

    pub(crate) fn read_u64(&mut self) -> Result<u64, DecodeError> {
        Ok(u64::from_le_bytes(self.read_array()?))
    }

    pub(crate) fn read_f32(&mut self) -> Result<f32, DecodeError> {
        Ok(f32::from_le_bytes(self.read_array()?))
    }

    /// A handle that may not be absent. Zero bits are what
    /// [`Handle::from_bits`] rejects, and no real handle ever has them.
    pub(crate) fn read_handle<T>(&mut self, field: &'static str) -> Result<Handle<T>, DecodeError> {
        Handle::from_bits(self.read_u64()?).ok_or(DecodeError::NullHandle { field })
    }

    /// A handle that may be absent, as a bare `u64` with zero for `None`.
    pub(crate) fn read_opt_handle<T>(&mut self) -> Result<Option<Handle<T>>, DecodeError> {
        Ok(Handle::from_bits(self.read_u64()?))
    }

    /// A [`Features`] word, through `from_bits` and never
    /// `from_bits_truncate`.
    ///
    /// The house rule for bitflags, and it matters in both directions for the
    /// same reason and opposite consequences: truncating a word from a build
    /// with more flags would report a *lesser* device on the reply side, and
    /// would quietly turn a required feature into one nobody asked for on the
    /// command side. Neither is anything a log would show.
    pub(crate) fn read_features(&mut self, field: &'static str) -> Result<Features, DecodeError> {
        let bits = self.read_u64()?;
        Features::from_bits(bits).ok_or(DecodeError::InvalidEnum { field, code: bits })
    }

    fn read_len(&mut self, field: &'static str, cap: usize) -> Result<usize, DecodeError> {
        let len = self.read_u32()?;
        if len as usize > cap {
            return Err(DecodeError::InvalidLength { field, len });
        }
        Ok(len as usize)
    }

    /// A length-prefixed byte field, capped by [`tag::MAX_FIELD_BYTES`].
    pub(crate) fn read_field(&mut self, field: &'static str) -> Result<&'a [u8], DecodeError> {
        let len = self.read_len(field, tag::MAX_FIELD_BYTES)?;
        self.read_bytes(len)
    }

    pub(crate) fn read_string(&mut self, field: &'static str) -> Result<String, DecodeError> {
        let bytes = self.read_field(field)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| DecodeError::NotUtf8 { field })
    }

    /// A presence byte, then the string if there is one. `Some("")` and `None`
    /// are different values and stay different here.
    pub(crate) fn read_opt_string(
        &mut self,
        field: &'static str,
    ) -> Result<Option<String>, DecodeError> {
        if self.read_present(field)? {
            Ok(Some(self.read_string(field)?))
        } else {
            Ok(None)
        }
    }

    /// A presence byte. Anything but the two canonical values is refused rather
    /// than read as truthy.
    pub(crate) fn read_present(&mut self, field: &'static str) -> Result<bool, DecodeError> {
        match self.read_u8()? {
            tag::ABSENT => Ok(false),
            tag::PRESENT => Ok(true),
            code => Err(DecodeError::InvalidEnum {
                field,
                code: code.into(),
            }),
        }
    }

    pub(crate) fn read_bool(&mut self, field: &'static str) -> Result<bool, DecodeError> {
        self.read_present(field)
    }

    /// An element count, capped by [`tag::MAX_ELEMENT_COUNT`] *and* by the bytes
    /// left — every element costs at least one byte, so a count past that cannot
    /// be honest, and neither cap alone bounds the allocation on its own.
    pub(crate) fn read_count(&mut self, field: &'static str) -> Result<usize, DecodeError> {
        let count = self.read_len(field, tag::MAX_ELEMENT_COUNT)?;
        if count > self.remaining() {
            return Err(DecodeError::InvalidLength {
                field,
                len: count as u32,
            });
        }
        Ok(count)
    }

    /// A `u32`-word slice: a count of words, then that many little-endian words.
    ///
    /// The primitive [`ShaderModuleDesc::spirv`](crcbl_hal::ShaderModuleDesc::spirv)
    /// crosses on, and the one that meets the house style's "pad nothing" rule
    /// against SPIR-V's own four-byte alignment. The words are decoded one at a
    /// time through [`read_u32`](Self::read_u32) — the deliberate alignment
    /// exception `docs/plan/41-webgpu-stream.md` calls for, taken over a hidden
    /// realignment copy: the buffer is byte-addressed and packed, so the words do
    /// not start on a four-byte boundary, and reading them word by word from
    /// little-endian bytes is exact wherever they land. Absence is the empty
    /// slice, so an empty count is a real empty artifact rather than a sentinel.
    ///
    /// [`read_count`](Self::read_count) caps the word count by
    /// [`tag::MAX_ELEMENT_COUNT`] and by the bytes left; each word then costs four
    /// more, which [`read_u32`](Self::read_u32) bounds-checks in turn.
    pub(crate) fn read_words(&mut self, field: &'static str) -> Result<Vec<u32>, DecodeError> {
        let count = self.read_count(field)?;
        let mut words = Vec::with_capacity(count);
        for _ in 0..count {
            words.push(self.read_u32()?);
        }
        Ok(words)
    }

    /// A counted list of `(entry point, container)` pairs — the shape
    /// [`ShaderModuleDesc::dxil`](crcbl_hal::ShaderModuleDesc::dxil) crosses on,
    /// and the worst-shaped field on the seam: each element is a length-prefixed
    /// string *and* a length-prefixed byte slice, two variable-length leaves that
    /// a fixed-stride decoder cannot skip.
    ///
    /// The two leaves are read in the descriptor's order — the name first, then
    /// the container — each with its own `u32` length prefix, so a pair whose
    /// container is empty is a *present* pair with a truncated artifact rather than
    /// an absent one, and the empty list is the only absence. Both leaves reuse
    /// the same caps every other field on this stream does:
    /// [`read_string`](Self::read_string) and [`read_field`](Self::read_field) are
    /// each capped by [`tag::MAX_FIELD_BYTES`], and the pair count by
    /// [`read_count`](Self::read_count).
    pub(crate) fn read_dxil(
        &mut self,
        field: &'static str,
    ) -> Result<Vec<(String, Vec<u8>)>, DecodeError> {
        let count = self.read_count(field)?;
        let mut pairs = Vec::with_capacity(count);
        for _ in 0..count {
            let entry_point = self.read_string(field)?;
            let container = self.read_field(field)?.to_vec();
            pairs.push((entry_point, container));
        }
        Ok(pairs)
    }
}

// ── ByteWriter ────────────────────────────────────────────────────────────────

/// The buffer everything in this crate encodes into, one field at a time.
///
/// The mirror of [`ByteReader`], and shared for the same reason: the two
/// directions write the same shapes, and a `put_` that disagreed with its `read_`
/// would be a bug in a format neither compiler reads both halves of.
///
/// # Panics
///
/// [`put_bytes`](Self::put_bytes) and [`put_count`](Self::put_count) panic past
/// [`tag::MAX_FIELD_BYTES`] and [`tag::MAX_ELEMENT_COUNT`], which are the caps
/// the reader enforces — so nothing this crate encodes is something it would
/// refuse to decode, and a caller that hits one has a bug a silent truncation
/// would bury.
#[derive(Debug, Default)]
pub(crate) struct ByteWriter {
    buf: Vec<u8>,
}

impl ByteWriter {
    pub(crate) fn with_capacity(bytes: usize) -> Self {
        Self {
            buf: Vec::with_capacity(bytes),
        }
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        &self.buf
    }

    /// Drops the encoded bytes, **keeping the allocation** — which is what makes
    /// a per-frame buffer free of allocation after the first frame, and what
    /// keeps its address stable across a release.
    pub(crate) fn clear(&mut self) {
        self.buf.clear();
    }

    /// The magic and version word every buffer in this crate opens with; the
    /// counterpart of [`ByteReader::read_format_header`].
    pub(crate) fn put_format_header(&mut self, magic: &[u8; 8], version: u16) {
        self.buf.extend_from_slice(magic);
        self.put_u16(version);
    }

    pub(crate) fn put_u8(&mut self, value: u8) {
        self.buf.push(value);
    }

    pub(crate) fn put_u16(&mut self, value: u16) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn put_u32(&mut self, value: u32) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn put_i32(&mut self, value: i32) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn put_u64(&mut self, value: u64) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn put_f32(&mut self, value: f32) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn put_handle<T>(&mut self, handle: Handle<T>) {
        self.put_u64(handle.to_bits());
    }

    /// An optional handle is a bare `u64` with zero for `None`: `to_bits` is
    /// documented as never zero, which is what the generation's niche was for.
    pub(crate) fn put_opt_handle<T>(&mut self, handle: Option<Handle<T>>) {
        self.put_u64(handle.map_or(0, Handle::to_bits));
    }

    /// The counterpart of [`ByteReader::read_features`].
    pub(crate) fn put_features(&mut self, features: Features) {
        self.put_u64(features.bits());
    }

    pub(crate) fn put_count(&mut self, count: usize) {
        assert!(
            count <= tag::MAX_ELEMENT_COUNT,
            "{count} elements is past the stream's cap, which the reader enforces"
        );
        self.put_u32(count as u32);
    }

    pub(crate) fn put_bytes(&mut self, bytes: &[u8]) {
        assert!(
            bytes.len() <= tag::MAX_FIELD_BYTES,
            "a {} byte field is past the stream's cap, which the reader enforces",
            bytes.len()
        );
        self.put_u32(bytes.len() as u32);
        self.buf.extend_from_slice(bytes);
    }

    /// A presence byte, then the string if there is one — so `Some("")` and
    /// `None` stay distinguishable, which a bare length prefix cannot do.
    pub(crate) fn put_opt_str(&mut self, value: Option<&str>) {
        match value {
            None => self.put_u8(tag::ABSENT),
            Some(text) => {
                self.put_u8(tag::PRESENT);
                self.put_bytes(text.as_bytes());
            }
        }
    }

    /// A `bool` goes over as a presence byte, and the reader decodes it as one —
    /// so a value that is neither is refused rather than read as true.
    ///
    /// Spelled with the constants rather than `u8::from(value)`. Those agree
    /// today only because `PRESENT` happens to be 1, which is two independent
    /// decisions that look like one: an implementer reading the writer alone
    /// would pair it with a `!= 0` test on the far side, and the strictness the
    /// reader intends would be gone without either half changing.
    pub(crate) fn put_bool(&mut self, value: bool) {
        self.put_u8(if value { tag::PRESENT } else { tag::ABSENT });
    }

    /// The counterpart of [`ByteReader::read_words`]: a word count, then that many
    /// little-endian words.
    ///
    /// The count is written through [`put_count`](Self::put_count), which asserts
    /// against [`tag::MAX_ELEMENT_COUNT`], so nothing this crate encodes is a word
    /// slice its own reader would refuse. Empty stays empty — the absence spelling
    /// for a word slice is the empty list, not a sentinel.
    pub(crate) fn put_words(&mut self, words: &[u32]) {
        self.put_count(words.len());
        for word in words {
            self.put_u32(*word);
        }
    }

    /// The counterpart of [`ByteReader::read_dxil`]: a pair count, then each pair
    /// as a length-prefixed name and a length-prefixed container.
    ///
    /// The name and the container each go over through [`put_bytes`](Self::put_bytes),
    /// which asserts against [`tag::MAX_FIELD_BYTES`], and the count through
    /// [`put_count`](Self::put_count). A pair whose container is empty is written
    /// as a present pair with a zero-length byte field, so the reader recovers a
    /// truncated artifact rather than an absent one — the distinction the empty
    /// *list* is what carries.
    pub(crate) fn put_dxil(&mut self, dxil: &[(&str, &[u8])]) {
        self.put_count(dxil.len());
        for (entry_point, container) in dxil {
            self.put_bytes(entry_point.as_bytes());
            self.put_bytes(container);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_read_past_the_end_reports_what_it_wanted_and_what_was_left() {
        let mut reader = ByteReader::new(&[1, 2, 3]);
        assert_eq!(reader.read_u16(), Ok(0x0201));
        assert_eq!(
            reader.read_u32(),
            Err(DecodeError::TooShort {
                needed: 4,
                offset: 2,
                remaining: 1,
            })
        );
        // A failed read consumes nothing.
        assert_eq!(reader.remaining(), 1);
        assert_eq!(reader.read_u8(), Ok(3));
        assert!(reader.is_empty());
    }

    #[test]
    fn a_null_handle_is_refused_where_none_is_not_a_value() {
        let zero = 0u64.to_le_bytes();
        let mut reader = ByteReader::new(&zero);
        assert_eq!(
            reader.read_handle::<()>("field"),
            Err(DecodeError::NullHandle { field: "field" })
        );

        // …and read as `None` where it is.
        let mut reader = ByteReader::new(&zero);
        assert_eq!(reader.read_opt_handle::<()>(), Ok(None));
    }

    #[test]
    fn a_presence_byte_that_is_neither_value_is_refused_rather_than_read_as_true() {
        let mut reader = ByteReader::new(&[2]);
        assert_eq!(
            reader.read_present("field"),
            Err(DecodeError::InvalidEnum {
                field: "field",
                code: 2
            })
        );
    }

    /// The header check is shared by both directions, so it has to refuse the
    /// *other* direction's magic — a reply buffer handed to the command reader
    /// is a shim wiring the two channels the wrong way round, and it must not
    /// decode as far as a tag.
    #[test]
    fn a_header_written_for_one_direction_is_refused_by_the_other() {
        let mut writer = ByteWriter::default();
        writer.put_format_header(tag::REPLY_MAGIC, tag::REPLY_VERSION);
        let replies = writer.bytes().to_vec();

        let mut reader = ByteReader::new(&replies);
        assert_eq!(
            reader.read_format_header(tag::STREAM_MAGIC, tag::STREAM_VERSION),
            Err(DecodeError::BadMagic)
        );

        let mut reader = ByteReader::new(&replies);
        assert_eq!(
            reader.read_format_header(tag::REPLY_MAGIC, tag::REPLY_VERSION),
            Ok(())
        );
        assert!(reader.is_empty(), "the header is all that was written");
    }

    #[test]
    fn each_decode_error_prints_the_field_and_offset_that_explain_it() {
        assert_eq!(
            DecodeError::TooShort {
                needed: 8,
                offset: 1,
                remaining: 3
            }
            .to_string(),
            "stream too short: need 8 bytes at offset 1, have 3"
        );
        assert_eq!(
            DecodeError::UnknownTag { tag: 0xAB }.to_string(),
            "unknown tag: 0xab"
        );
        assert_eq!(
            DecodeError::UnsupportedVersion {
                found: 2,
                expected: 1
            }
            .to_string(),
            "stream format version 2, expected 1"
        );
        assert_eq!(
            DecodeError::InvalidLength {
                field: "PushConstants::data",
                len: 999
            }
            .to_string(),
            "invalid length for PushConstants::data: 999"
        );
        assert_eq!(
            DecodeError::NullHandle {
                field: "BindGroup::layout"
            }
            .to_string(),
            "BindGroup::layout is a null handle"
        );
        assert_eq!(
            DecodeError::UnexpectedSequence { sequence: 42 }.to_string(),
            "nothing is waiting on a reply for sequence 42"
        );
    }
}
