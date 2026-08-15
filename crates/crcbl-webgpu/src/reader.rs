//! The decoder: a stream buffer in, [`Command`]s out, never a panic.
//!
//! # This is not the production decoder
//!
//! JS is. A stream is replayed by a JavaScript function reading the wasm heap in
//! place, and nothing in this module runs in a browser. What it is for:
//!
//! * **Testing the encoding without one.** Round-tripping every command shape
//!   through Rust is what catches a field written in the wrong order, a length
//!   prefix that counts the wrong thing, or an argument dropped from the end of
//!   a call — none of which the encoder can catch alone, and all of which would
//!   otherwise surface as a WebGPU validation error in a browser.
//! * **Dumping a stream.** [`Command::name`] plus a `Debug` print turns a buffer
//!   into a readable list of what the frame asked for.
//!
//! It therefore has to be *strict* rather than forgiving: every bound is
//! checked, every enum code is one this crate wrote, and a stream this decoder
//! accepts is one the JS replayer can be held to.

use crcbl_core::Handle;
use crcbl_hal::{
    BufferUsage, ClearValue, ColorAttachment, DepthStencilAttachment, LoadOp, Rect2d, ShaderStages,
    StoreOp,
};

use crate::{Command, tag};

// ── Error type ────────────────────────────────────────────────────────────────

/// Everything a malformed stream can be.
///
/// Shaped after [`crcbl_net`'s `DecodeError`](https://docs.rs/crcbl-net), which
/// is the house wire style: the variants say where the decode stopped and what
/// it wanted, so a report names a field rather than a buffer.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    /// The buffer does not start with [`tag::STREAM_MAGIC`].
    #[error("not a command stream: bad magic")]
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
    /// A tag byte no command in [`tag`] claims. Distinguishable from a malformed
    /// known command precisely because the tag comes first.
    #[error("unknown command tag: 0x{tag:02x}")]
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
        /// The code that was read.
        code: u32,
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
}

// ── ByteReader ────────────────────────────────────────────────────────────────

/// Cursor over a stream buffer that bounds-checks every read.
///
/// Every decode below goes through this rather than hand-rolling
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
    fn read_handle<T>(&mut self, field: &'static str) -> Result<Handle<T>, DecodeError> {
        Handle::from_bits(self.read_u64()?).ok_or(DecodeError::NullHandle { field })
    }

    /// A handle that may be absent, as a bare `u64` with zero for `None`.
    fn read_opt_handle<T>(&mut self) -> Result<Option<Handle<T>>, DecodeError> {
        Ok(Handle::from_bits(self.read_u64()?))
    }

    fn read_len(&mut self, field: &'static str, cap: usize) -> Result<usize, DecodeError> {
        let len = self.read_u32()?;
        if len as usize > cap {
            return Err(DecodeError::InvalidLength { field, len });
        }
        Ok(len as usize)
    }

    /// A length-prefixed byte field, capped by [`tag::MAX_FIELD_BYTES`].
    fn read_field(&mut self, field: &'static str) -> Result<&'a [u8], DecodeError> {
        let len = self.read_len(field, tag::MAX_FIELD_BYTES)?;
        self.read_bytes(len)
    }

    fn read_string(&mut self, field: &'static str) -> Result<String, DecodeError> {
        let bytes = self.read_field(field)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| DecodeError::NotUtf8 { field })
    }

    /// A presence byte, then the string if there is one. `Some("")` and `None`
    /// are different values and stay different here.
    fn read_opt_string(&mut self, field: &'static str) -> Result<Option<String>, DecodeError> {
        if self.read_present(field)? {
            Ok(Some(self.read_string(field)?))
        } else {
            Ok(None)
        }
    }

    /// A presence byte. Anything but the two canonical values is refused rather
    /// than read as truthy.
    fn read_present(&mut self, field: &'static str) -> Result<bool, DecodeError> {
        match self.read_u8()? {
            tag::ABSENT => Ok(false),
            tag::PRESENT => Ok(true),
            code => Err(DecodeError::InvalidEnum {
                field,
                code: code.into(),
            }),
        }
    }

    fn read_bool(&mut self, field: &'static str) -> Result<bool, DecodeError> {
        self.read_present(field)
    }

    /// An element count, capped by [`tag::MAX_ELEMENT_COUNT`] *and* by the bytes
    /// left — every element costs at least one byte, so a count past that cannot
    /// be honest, and neither cap alone bounds the allocation on its own.
    fn read_count(&mut self, field: &'static str) -> Result<usize, DecodeError> {
        let count = self.read_len(field, tag::MAX_ELEMENT_COUNT)?;
        if count > self.remaining() {
            return Err(DecodeError::InvalidLength {
                field,
                len: count as u32,
            });
        }
        Ok(count)
    }

    fn read_load_op(&mut self, field: &'static str) -> Result<LoadOp, DecodeError> {
        let code = self.read_u8()?;
        tag::load_op_from_code(code).ok_or(DecodeError::InvalidEnum {
            field,
            code: code.into(),
        })
    }

    fn read_store_op(&mut self, field: &'static str) -> Result<StoreOp, DecodeError> {
        let code = self.read_u8()?;
        tag::store_op_from_code(code).ok_or(DecodeError::InvalidEnum {
            field,
            code: code.into(),
        })
    }

    fn read_clear_value(&mut self) -> Result<ClearValue, DecodeError> {
        Ok(ClearValue {
            color: [
                self.read_f32()?,
                self.read_f32()?,
                self.read_f32()?,
                self.read_f32()?,
            ],
            depth: self.read_f32()?,
            stencil: self.read_u32()?,
        })
    }

    fn read_rect(&mut self) -> Result<Rect2d, DecodeError> {
        Ok(Rect2d {
            x: self.read_i32()?,
            y: self.read_i32()?,
            width: self.read_u32()?,
            height: self.read_u32()?,
        })
    }

    fn read_color_attachment(&mut self) -> Result<ColorAttachment, DecodeError> {
        Ok(ColorAttachment {
            view: self.read_handle("ColorAttachment::view")?,
            resolve: self.read_opt_handle()?,
            load: self.read_load_op("ColorAttachment::load")?,
            store: self.read_store_op("ColorAttachment::store")?,
            clear: self.read_clear_value()?,
        })
    }

    fn read_depth_stencil_attachment(&mut self) -> Result<DepthStencilAttachment, DecodeError> {
        Ok(DepthStencilAttachment {
            view: self.read_handle("DepthStencilAttachment::view")?,
            read_only: self.read_bool("DepthStencilAttachment::read_only")?,
            depth_load: self.read_load_op("DepthStencilAttachment::depth_load")?,
            depth_store: self.read_store_op("DepthStencilAttachment::depth_store")?,
            stencil_load: self.read_load_op("DepthStencilAttachment::stencil_load")?,
            stencil_store: self.read_store_op("DepthStencilAttachment::stencil_store")?,
            clear: self.read_clear_value()?,
        })
    }
}

// ── StreamReader ──────────────────────────────────────────────────────────────

/// Decodes a buffer [`StreamWriter`](crate::StreamWriter) produced.
///
/// See the [module docs](self) for what this is and is not: it exists so the
/// encoding is testable without a browser, and so a stream can be dumped. The
/// decoder JS replays with is a separate implementation of the same format.
#[derive(Debug)]
pub struct StreamReader<'a> {
    reader: ByteReader<'a>,
    base_sequence: u64,
    decoded: u64,
    /// Set once a decode fails. Nothing is resumable after that: the cursor is
    /// somewhere inside a command body and the next byte is not a tag.
    failed: bool,
}

impl<'a> StreamReader<'a> {
    /// Opens a stream, checking its magic and version.
    ///
    /// # Errors
    ///
    /// [`DecodeError::BadMagic`] or [`DecodeError::UnsupportedVersion`] if the
    /// header is not this format's, [`DecodeError::TooShort`] if there is not a
    /// whole header.
    pub fn new(stream: &'a [u8]) -> Result<Self, DecodeError> {
        let mut reader = ByteReader::new(stream);
        if reader.read_bytes(tag::STREAM_MAGIC.len())? != tag::STREAM_MAGIC {
            return Err(DecodeError::BadMagic);
        }
        let version = reader.read_u16()?;
        if version != tag::STREAM_VERSION {
            return Err(DecodeError::UnsupportedVersion {
                found: version,
                expected: tag::STREAM_VERSION,
            });
        }
        let base_sequence = reader.read_u64()?;
        Ok(Self {
            reader,
            base_sequence,
            decoded: 0,
            failed: false,
        })
    }

    /// The sequence number of the first command in this buffer.
    #[must_use]
    pub const fn base_sequence(&self) -> u64 {
        self.base_sequence
    }

    /// The next command and the sequence number it carries, or `None` at the end
    /// of the stream.
    ///
    /// Sequence numbers are positional — the *n*th command decoded is
    /// [`base_sequence`](Self::base_sequence)` + n` — which is why nothing per
    /// command is on the wire. See [`StreamWriter`](crate::StreamWriter).
    ///
    /// Returns `None` forever after an error: the cursor is then somewhere
    /// inside a command body, so the next byte is not a tag and resuming would
    /// invent commands out of a payload.
    ///
    /// # Errors
    ///
    /// Any [`DecodeError`] the command body produces.
    pub fn next_command(&mut self) -> Option<Result<(u64, Command), DecodeError>> {
        if self.failed || self.reader.is_empty() {
            return None;
        }
        match self.decode_command() {
            Ok(command) => {
                // Wrapping because the base came off the wire: a buffer
                // declaring `u64::MAX` would otherwise panic the decoder in a
                // debug build, which is the one thing this must never do.
                let sequence = self.base_sequence.wrapping_add(self.decoded);
                self.decoded += 1;
                Some(Ok((sequence, command)))
            }
            Err(error) => {
                self.failed = true;
                Some(Err(error))
            }
        }
    }

    fn decode_command(&mut self) -> Result<Command, DecodeError> {
        let r = &mut self.reader;
        let opcode = r.read_u8()?;
        match opcode {
            tag::CREATE_BUFFER_TAG => {
                let buffer = r.read_handle("CreateBuffer::buffer")?;
                let label = r.read_opt_string("BufferDesc::label")?;
                let size = r.read_u64()?;
                let usage_bits = r.read_u32()?;
                let usage = BufferUsage::from_bits(usage_bits).ok_or(DecodeError::InvalidEnum {
                    field: "BufferDesc::usage",
                    code: usage_bits,
                })?;
                let memory_code = r.read_u8()?;
                let memory = tag::memory_location_from_code(memory_code).ok_or(
                    DecodeError::InvalidEnum {
                        field: "BufferDesc::memory",
                        code: memory_code.into(),
                    },
                )?;
                Ok(Command::CreateBuffer {
                    buffer,
                    label,
                    size,
                    usage,
                    memory,
                })
            }
            tag::DESTROY_BUFFER_TAG => Ok(Command::DestroyBuffer {
                buffer: r.read_handle("DestroyBuffer::buffer")?,
            }),
            tag::BEGIN_DEBUG_LABEL_TAG => Ok(Command::BeginDebugLabel {
                label: r.read_string("BeginDebugLabel::label")?,
            }),
            tag::BEGIN_RENDER_PASS_TAG => {
                let label = r.read_opt_string("RenderPassDesc::label")?;
                let count = r.read_count("RenderPassDesc::color_attachments")?;
                let mut color_attachments = Vec::with_capacity(count);
                for _ in 0..count {
                    color_attachments.push(r.read_color_attachment()?);
                }
                let depth_stencil_attachment =
                    if r.read_present("RenderPassDesc::depth_stencil_attachment")? {
                        Some(r.read_depth_stencil_attachment()?)
                    } else {
                        None
                    };
                Ok(Command::BeginRenderPass {
                    label,
                    color_attachments,
                    depth_stencil_attachment,
                    render_area: r.read_rect()?,
                })
            }
            tag::BIND_GRAPHICS_PIPELINE_TAG => Ok(Command::BindGraphicsPipeline {
                pipeline: r.read_handle("BindGraphicsPipeline::pipeline")?,
            }),
            tag::BIND_GROUP_TAG => {
                let slot = r.read_u32()?;
                let group = r.read_handle("BindGroup::group")?;
                let count = r.read_count("BindGroup::dynamic_offsets")?;
                let mut dynamic_offsets = Vec::with_capacity(count);
                for _ in 0..count {
                    dynamic_offsets.push(r.read_u32()?);
                }
                Ok(Command::BindGroup {
                    slot,
                    group,
                    dynamic_offsets,
                    layout: r.read_handle("BindGroup::layout")?,
                })
            }
            tag::PUSH_CONSTANTS_TAG => {
                let stage_bits = r.read_u32()?;
                let stages =
                    ShaderStages::from_bits(stage_bits).ok_or(DecodeError::InvalidEnum {
                        field: "PushConstants::stages",
                        code: stage_bits,
                    })?;
                let offset = r.read_u32()?;
                let data = r.read_field("PushConstants::data")?.to_vec();
                Ok(Command::PushConstants {
                    stages,
                    offset,
                    data,
                    layout: r.read_handle("PushConstants::layout")?,
                })
            }
            tag::DRAW_TAG => {
                // Spelled out rather than `r.read_u32()?..r.read_u32()?`: the
                // range's two halves are read in source order either way, but
                // relying on that to get the field order right reads as a
                // coincidence.
                let first_vertex = r.read_u32()?;
                let last_vertex = r.read_u32()?;
                let first_instance = r.read_u32()?;
                let last_instance = r.read_u32()?;
                Ok(Command::Draw {
                    vertices: first_vertex..last_vertex,
                    instances: first_instance..last_instance,
                })
            }
            unknown => Err(DecodeError::UnknownTag { tag: unknown }),
        }
    }
}

/// Every command in a stream, in order.
///
/// The convenience half of [`StreamReader`], for a test or a dump that wants the
/// whole buffer at once. The sequence numbers are
/// [`StreamReader::base_sequence`] plus each command's index.
///
/// # Errors
///
/// The first [`DecodeError`] the stream produces; nothing after it is decoded.
pub fn decode_stream(stream: &[u8]) -> Result<Vec<Command>, DecodeError> {
    let mut reader = StreamReader::new(stream)?;
    let mut commands = Vec::new();
    while let Some(next) = reader.next_command() {
        commands.push(next?.1);
    }
    Ok(commands)
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
            "unknown command tag: 0xab"
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
    }
}
