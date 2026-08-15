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

use crcbl_hal::{
    AdapterId, BufferUsage, ClearValue, ColorAttachment, DepthStencilAttachment, LoadOp, Rect2d,
    ShaderStages, StoreOp,
};

use crate::bytes::{ByteReader, DecodeError};
use crate::{Command, tag};

// ── The command stream's own field readers ────────────────────────────────────
//
// [`ByteReader`] and its primitives live in [`crate::bytes`], shared with the
// reply direction. What is below is shaped by `crcbl-hal`'s descriptors and is
// the command stream's alone, so it is an impl block here rather than one more
// thing the reply reader has to read past.

impl ByteReader<'_> {
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
        reader.read_format_header(tag::STREAM_MAGIC, tag::STREAM_VERSION)?;
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
                    code: usage_bits.into(),
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
            tag::CREATE_SURFACE_TAG => {
                let surface = r.read_handle("CreateSurface::surface")?;
                let canvas_id = r.read_u32()?;
                Ok(Command::CreateSurface { surface, canvas_id })
            }
            tag::DESTROY_BUFFER_TAG => Ok(Command::DestroyBuffer {
                buffer: r.read_handle("DestroyBuffer::buffer")?,
            }),
            tag::DESTROY_SURFACE_TAG => Ok(Command::DestroySurface {
                surface: r.read_handle("DestroySurface::surface")?,
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
                let render_area = r.read_rect()?;
                Ok(Command::BeginRenderPass {
                    label,
                    color_attachments,
                    depth_stencil_attachment,
                    render_area,
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
                let layout = r.read_handle("BindGroup::layout")?;
                Ok(Command::BindGroup {
                    slot,
                    group,
                    dynamic_offsets,
                    layout,
                })
            }
            tag::PUSH_CONSTANTS_TAG => {
                let stage_bits = r.read_u32()?;
                let stages =
                    ShaderStages::from_bits(stage_bits).ok_or(DecodeError::InvalidEnum {
                        field: "PushConstants::stages",
                        code: stage_bits.into(),
                    })?;
                let offset = r.read_u32()?;
                let data = r.read_field("PushConstants::data")?.to_vec();
                let layout = r.read_handle("PushConstants::layout")?;
                Ok(Command::PushConstants {
                    stages,
                    offset,
                    data,
                    layout,
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
            tag::ENUMERATE_ADAPTERS_TAG => Ok(Command::EnumerateAdapters),
            tag::REQUEST_DEVICE_TAG => {
                let adapter = AdapterId(r.read_u32()?);
                let label = r.read_opt_string("DeviceDesc::label")?;
                let required_features = r.read_features("DeviceDesc::required_features")?;
                let optional_features = r.read_features("DeviceDesc::optional_features")?;
                let compatible_surface = r.read_opt_handle()?;
                Ok(Command::RequestDevice {
                    adapter,
                    label,
                    required_features,
                    optional_features,
                    compatible_surface,
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

    /// The `ByteReader` unit tests moved to [`crate::bytes`] with the type; what
    /// belongs here is the part of the decode this module owns. The header is
    /// the smallest of those: `new` reads the base sequence after the shared
    /// magic-and-version check, and a buffer that stops between the two is short
    /// rather than corrupt.
    #[test]
    fn a_header_missing_its_base_sequence_is_too_short_rather_than_a_stream() {
        let mut header = Vec::new();
        header.extend_from_slice(tag::STREAM_MAGIC);
        header.extend_from_slice(&tag::STREAM_VERSION.to_le_bytes());
        assert_eq!(header.len(), tag::HEADER_BYTES - 8);
        assert!(matches!(
            StreamReader::new(&header),
            Err(DecodeError::TooShort { needed: 8, .. })
        ));

        header.extend_from_slice(&7u64.to_le_bytes());
        let reader = StreamReader::new(&header).expect("a whole header is a stream");
        assert_eq!(reader.base_sequence(), 7);
    }
}
