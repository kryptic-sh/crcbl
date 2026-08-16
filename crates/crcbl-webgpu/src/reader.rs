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
    AdapterId, BindGroupEntry, BindGroupLayoutEntry, BindingFlags, BindingKind, BindingResource,
    BufferUsage, ClearValue, ColorAttachment, CompareOp, DepthStencilAttachment, Extent3d,
    FilterMode, Format, ImageAspect, ImageSubresourceRange, ImageType, ImageUsage, ImageViewType,
    LoadOp, Rect2d, SampleType, SamplerAddressMode, ShaderStages, StoreOp,
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

    fn read_image_type(&mut self, field: &'static str) -> Result<ImageType, DecodeError> {
        let code = self.read_u8()?;
        tag::image_type_from_code(code).ok_or(DecodeError::InvalidEnum {
            field,
            code: code.into(),
        })
    }

    fn read_image_view_type(&mut self, field: &'static str) -> Result<ImageViewType, DecodeError> {
        let code = self.read_u8()?;
        tag::image_view_type_from_code(code).ok_or(DecodeError::InvalidEnum {
            field,
            code: code.into(),
        })
    }

    fn read_filter_mode(&mut self, field: &'static str) -> Result<FilterMode, DecodeError> {
        let code = self.read_u8()?;
        tag::filter_mode_from_code(code).ok_or(DecodeError::InvalidEnum {
            field,
            code: code.into(),
        })
    }

    fn read_sampler_address_mode(
        &mut self,
        field: &'static str,
    ) -> Result<SamplerAddressMode, DecodeError> {
        let code = self.read_u8()?;
        tag::sampler_address_mode_from_code(code).ok_or(DecodeError::InvalidEnum {
            field,
            code: code.into(),
        })
    }

    /// The presence byte, then the code if there is one.
    ///
    /// The first optional enum on this stream, and it is two reads rather than a
    /// widened table: a byte that is neither presence value is refused by
    /// [`read_present`](Self::read_present) naming the field, and a code no
    /// variant claims is refused here naming the same one — two failures a
    /// reserved "absent" code inside the table could not tell apart.
    fn read_opt_compare_op(
        &mut self,
        field: &'static str,
    ) -> Result<Option<CompareOp>, DecodeError> {
        if !self.read_present(field)? {
            return Ok(None);
        }
        let code = self.read_u8()?;
        tag::compare_op_from_code(code)
            .map(Some)
            .ok_or(DecodeError::InvalidEnum {
                field,
                code: code.into(),
            })
    }

    fn read_sample_type(&mut self, field: &'static str) -> Result<SampleType, DecodeError> {
        let code = self.read_u8()?;
        tag::sample_type_from_code(code).ok_or(DecodeError::InvalidEnum {
            field,
            code: code.into(),
        })
    }

    /// A [`BindingKind`] code and that variant's own fields.
    ///
    /// **The dispatch lives here rather than in [`crate::tag`]**, and that is
    /// the shape of the type rather than a preference: every other table there
    /// answers `Option<Variant>` from a byte, and this enum's variants carry
    /// data, so there is no `BindingKind` to answer with until the body has been
    /// read. What `tag` keeps is the writer's half — the codes and
    /// [`tag::binding_kind_code`]'s exhaustive `match`, which is what stops
    /// compiling when a variant is added.
    ///
    /// The refusing arm is therefore this function's, and it refuses rather than
    /// folding for a reason particular to this table: the bodies are different
    /// lengths. A code read as its neighbour does not merely mis-name the
    /// binding — it consumes the wrong number of bytes and lands the cursor
    /// inside the next entry, so every field after it is noise that still
    /// decodes.
    fn read_binding_kind(&mut self, field: &'static str) -> Result<BindingKind, DecodeError> {
        let code = self.read_u8()?;
        match code {
            tag::BINDING_KIND_UNIFORM_BUFFER => Ok(BindingKind::UniformBuffer {
                dynamic: self.read_bool("BindingKind::dynamic")?,
            }),
            tag::BINDING_KIND_STORAGE_BUFFER => Ok(BindingKind::StorageBuffer {
                read_only: self.read_bool("BindingKind::read_only")?,
                dynamic: self.read_bool("BindingKind::dynamic")?,
            }),
            tag::BINDING_KIND_SAMPLED_IMAGE => Ok(BindingKind::SampledImage {
                view_type: self.read_image_view_type("BindingKind::view_type")?,
                sample_type: self.read_sample_type("BindingKind::sample_type")?,
            }),
            tag::BINDING_KIND_STORAGE_IMAGE => Ok(BindingKind::StorageImage {
                read_only: self.read_bool("BindingKind::read_only")?,
            }),
            tag::BINDING_KIND_SAMPLER => Ok(BindingKind::Sampler {
                comparison: self.read_bool("BindingKind::comparison")?,
            }),
            _ => Err(DecodeError::InvalidEnum {
                field,
                code: code.into(),
            }),
        }
    }

    /// A [`ShaderStages`] word, through `from_bits` and never
    /// `from_bits_truncate` — the rule every bitflags field on this stream
    /// follows, and here the cost of breaking it is a *narrower layout*: a
    /// binding the caller declared visible to a stage this build has no name for
    /// would arrive invisible to it, and the shader compiled against it would
    /// read whatever the slot happened to hold.
    fn read_shader_stages(&mut self, field: &'static str) -> Result<ShaderStages, DecodeError> {
        let bits = self.read_u32()?;
        ShaderStages::from_bits(bits).ok_or(DecodeError::InvalidEnum {
            field,
            code: bits.into(),
        })
    }

    /// A [`BindingFlags`] word, on [`read_shader_stages`](Self::read_shader_stages)'s
    /// terms.
    ///
    /// `BindingFlags`'s own docs make the strictness obligatory rather than
    /// stylistic: "a backend without it must reject a layout that sets any of
    /// them rather than silently ignoring it — a bindless array quietly
    /// downgraded to a fixed one reads garbage at index 4097." Truncating an
    /// unclaimed bit here is that downgrade, one layer earlier.
    fn read_binding_flags(&mut self, field: &'static str) -> Result<BindingFlags, DecodeError> {
        let bits = self.read_u32()?;
        BindingFlags::from_bits(bits).ok_or(DecodeError::InvalidEnum {
            field,
            code: bits.into(),
        })
    }

    /// One [`BindGroupLayoutEntry`], in the order the struct declares its fields.
    ///
    /// Spelled out one field at a time rather than built inline for
    /// `CreateSampler`'s reason: `binding` and `count` are adjacent-in-meaning
    /// `u32`s that both hold small numbers, and a body that read them in the
    /// other order still decodes to an entry.
    fn read_bind_group_layout_entry(&mut self) -> Result<BindGroupLayoutEntry, DecodeError> {
        let binding = self.read_u32()?;
        let visibility = self.read_shader_stages("BindGroupLayoutEntry::visibility")?;
        let kind = self.read_binding_kind("BindGroupLayoutEntry::kind")?;
        let count = self.read_u32()?;
        let flags = self.read_binding_flags("BindGroupLayoutEntry::flags")?;
        Ok(BindGroupLayoutEntry {
            binding,
            visibility,
            kind,
            count,
            flags,
        })
    }

    /// A [`BindingResource`] code and that variant's own fields.
    ///
    /// The dispatch lives here rather than in [`crate::tag`] for
    /// [`read_binding_kind`](Self::read_binding_kind)'s reason — the enum carries
    /// data, so there is no [`BindingResource`] to answer with until the body has
    /// been read — and the refusing arm is this function's for the same reason
    /// with a sharper edge: the bodies are different lengths *and* a
    /// [`BindingResource::Buffer`] and a
    /// [`BindingResource::Sampler`] both resolve a handle only the discriminant
    /// says the table for, so a folded code binds the wrong kind of object as well
    /// as landing the cursor wrong. See
    /// [`tag::binding_resource_code`](crate::tag::binding_resource_code).
    fn read_binding_resource(
        &mut self,
        field: &'static str,
    ) -> Result<BindingResource, DecodeError> {
        let code = self.read_u8()?;
        match code {
            tag::BINDING_RESOURCE_BUFFER => Ok(BindingResource::Buffer {
                buffer: self.read_handle("BindingResource::buffer")?,
                offset: self.read_u64()?,
                size: self.read_u64()?,
            }),
            tag::BINDING_RESOURCE_IMAGE_VIEW => Ok(BindingResource::ImageView(
                self.read_handle("BindingResource::view")?,
            )),
            tag::BINDING_RESOURCE_SAMPLER => Ok(BindingResource::Sampler(
                self.read_handle("BindingResource::sampler")?,
            )),
            _ => Err(DecodeError::InvalidEnum {
                field,
                code: code.into(),
            }),
        }
    }

    /// One [`BindGroupEntry`], in the order the struct declares its fields.
    ///
    /// Spelled out one field at a time rather than built inline for
    /// [`read_bind_group_layout_entry`](Self::read_bind_group_layout_entry)'s
    /// reason: `binding` and `array_index` are adjacent `u32`s that both hold
    /// small numbers, and a body that read them in the other order still decodes
    /// to an entry.
    fn read_bind_group_entry(&mut self) -> Result<BindGroupEntry, DecodeError> {
        let binding = self.read_u32()?;
        let array_index = self.read_u32()?;
        let resource = self.read_binding_resource("BindGroupEntry::resource")?;
        Ok(BindGroupEntry {
            binding,
            array_index,
            resource,
        })
    }

    fn read_format(&mut self, field: &'static str) -> Result<Format, DecodeError> {
        let code = self.read_u8()?;
        tag::format_from_code(code).ok_or(DecodeError::InvalidEnum {
            field,
            code: code.into(),
        })
    }

    /// A bitflags word, through `from_bits` and never `from_bits_truncate`: a
    /// bit no flag claims is a build that knows a use this one does not, and
    /// dropping it would create an image the caller cannot bind.
    fn read_image_usage(&mut self, field: &'static str) -> Result<ImageUsage, DecodeError> {
        let bits = self.read_u32()?;
        ImageUsage::from_bits(bits).ok_or(DecodeError::InvalidEnum {
            field,
            code: bits.into(),
        })
    }

    fn read_image_aspect(&mut self, field: &'static str) -> Result<ImageAspect, DecodeError> {
        let bits = self.read_u32()?;
        ImageAspect::from_bits(bits).ok_or(DecodeError::InvalidEnum {
            field,
            code: bits.into(),
        })
    }

    fn read_extent(&mut self) -> Result<Extent3d, DecodeError> {
        Ok(Extent3d {
            width: self.read_u32()?,
            height: self.read_u32()?,
            depth_or_layers: self.read_u32()?,
        })
    }

    fn read_subresource_range(&mut self) -> Result<ImageSubresourceRange, DecodeError> {
        Ok(ImageSubresourceRange {
            aspect: self.read_image_aspect("ImageSubresourceRange::aspect")?,
            base_mip: self.read_u32()?,
            mip_count: self.read_u32()?,
            base_layer: self.read_u32()?,
            layer_count: self.read_u32()?,
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
            tag::CREATE_IMAGE_TAG => {
                // Spelled out rather than built inline: `mip_levels` and
                // `samples` are adjacent, identically typed `u32`s and mean
                // different things, which is the pair a decoder most easily
                // swaps. Both are carried verbatim, zero included — see
                // `Command::CreateImage`.
                let image = r.read_handle("CreateImage::image")?;
                let label = r.read_opt_string("ImageDesc::label")?;
                let image_type = r.read_image_type("ImageDesc::image_type")?;
                let extent = r.read_extent()?;
                let format = r.read_format("ImageDesc::format")?;
                let mip_levels = r.read_u32()?;
                let samples = r.read_u32()?;
                let usage = r.read_image_usage("ImageDesc::usage")?;
                Ok(Command::CreateImage {
                    image,
                    label,
                    image_type,
                    extent,
                    format,
                    mip_levels,
                    samples,
                    usage,
                })
            }
            tag::CREATE_IMAGE_VIEW_TAG => {
                // Two handles, and the id being filled in comes first — the
                // one being read is a field of the descriptor and follows the
                // label, as it does in `ImageViewDesc`.
                let view = r.read_handle("CreateImageView::view")?;
                let label = r.read_opt_string("ImageViewDesc::label")?;
                let image = r.read_handle("ImageViewDesc::image")?;
                let view_type = r.read_image_view_type("ImageViewDesc::view_type")?;
                let format = r.read_format("ImageViewDesc::format")?;
                let range = r.read_subresource_range()?;
                Ok(Command::CreateImageView {
                    view,
                    label,
                    image,
                    view_type,
                    format,
                    range,
                })
            }
            tag::CREATE_SAMPLER_TAG => {
                // Spelled out one field at a time rather than built inline, for
                // `CreateImage`'s reason with six fields instead of two: three
                // `FilterMode` bytes and three `SamplerAddressMode` bytes go
                // over back to back, and any two of the six read in the wrong
                // order still decodes to a sampler.
                let sampler = r.read_handle("CreateSampler::sampler")?;
                let label = r.read_opt_string("SamplerDesc::label")?;
                let mag_filter = r.read_filter_mode("SamplerDesc::mag_filter")?;
                let min_filter = r.read_filter_mode("SamplerDesc::min_filter")?;
                let mip_filter = r.read_filter_mode("SamplerDesc::mip_filter")?;
                let address_mode = [
                    r.read_sampler_address_mode("SamplerDesc::address_mode")?,
                    r.read_sampler_address_mode("SamplerDesc::address_mode")?,
                    r.read_sampler_address_mode("SamplerDesc::address_mode")?,
                ];
                let lod_min = r.read_f32()?;
                let lod_max = r.read_f32()?;
                let anisotropy = r.read_f32()?;
                let compare = r.read_opt_compare_op("SamplerDesc::compare")?;
                Ok(Command::CreateSampler {
                    sampler,
                    label,
                    mag_filter,
                    min_filter,
                    mip_filter,
                    address_mode,
                    lod_min,
                    lod_max,
                    anisotropy,
                    compare,
                })
            }
            tag::CREATE_BIND_GROUP_LAYOUT_TAG => {
                let layout = r.read_handle("CreateBindGroupLayout::layout")?;
                let label = r.read_opt_string("BindGroupLayoutDesc::label")?;
                let count = r.read_count("BindGroupLayoutDesc::entries")?;
                // Pushed in wire order and never sorted: the slice's order is
                // part of the descriptor's meaning, not a presentation of it.
                // See `Command::CreateBindGroupLayout`.
                let mut entries = Vec::with_capacity(count);
                for _ in 0..count {
                    entries.push(r.read_bind_group_layout_entry()?);
                }
                Ok(Command::CreateBindGroupLayout {
                    layout,
                    label,
                    entries,
                })
            }
            tag::CREATE_BIND_GROUP_TAG => {
                let group = r.read_handle("CreateBindGroup::group")?;
                let label = r.read_opt_string("BindGroupDesc::label")?;
                let layout = r.read_handle("BindGroupDesc::layout")?;
                let count = r.read_count("BindGroupDesc::entries")?;
                // Pushed in wire order and never keyed by binding: an entry's
                // `array_index` is the bindless write path, so two entries may
                // share a binding. See `Command::CreateBindGroup`.
                let mut entries = Vec::with_capacity(count);
                for _ in 0..count {
                    entries.push(r.read_bind_group_entry()?);
                }
                // An optional scalar behind a presence byte, exactly as
                // `create_sampler`'s `compare` is.
                let variable_count = if r.read_present("BindGroupDesc::variable_count")? {
                    Some(r.read_u32()?)
                } else {
                    None
                };
                Ok(Command::CreateBindGroup {
                    group,
                    label,
                    layout,
                    entries,
                    variable_count,
                })
            }
            tag::CREATE_SHADER_MODULE_TAG => {
                // Every field of `ShaderModuleDesc`, in its declaration order, and
                // every one with its own absence convention preserved: `spirv`
                // empty is absent, `wgsl`/`msl` keep `Some("")` apart from `None`,
                // and `dxil`'s empty list is absence while a pair with an empty
                // container is a truncated artifact. A browser reads only `wgsl`,
                // but this decoder traverses all four to reach the command's end.
                let module = r.read_handle("CreateShaderModule::module")?;
                let label = r.read_opt_string("ShaderModuleDesc::label")?;
                let spirv = r.read_words("ShaderModuleDesc::spirv")?;
                let wgsl = r.read_opt_string("ShaderModuleDesc::wgsl")?;
                let msl = r.read_opt_string("ShaderModuleDesc::msl")?;
                let dxil = r.read_dxil("ShaderModuleDesc::dxil")?;
                Ok(Command::CreateShaderModule {
                    module,
                    label,
                    spirv,
                    wgsl,
                    msl,
                    dxil,
                })
            }
            tag::DESTROY_SHADER_MODULE_TAG => Ok(Command::DestroyShaderModule {
                module: r.read_handle("DestroyShaderModule::module")?,
            }),
            tag::DESTROY_BIND_GROUP_LAYOUT_TAG => Ok(Command::DestroyBindGroupLayout {
                layout: r.read_handle("DestroyBindGroupLayout::layout")?,
            }),
            tag::DESTROY_BIND_GROUP_TAG => Ok(Command::DestroyBindGroup {
                group: r.read_handle("DestroyBindGroup::group")?,
            }),
            tag::DESTROY_BUFFER_TAG => Ok(Command::DestroyBuffer {
                buffer: r.read_handle("DestroyBuffer::buffer")?,
            }),
            tag::DESTROY_SURFACE_TAG => Ok(Command::DestroySurface {
                surface: r.read_handle("DestroySurface::surface")?,
            }),
            tag::DESTROY_IMAGE_TAG => Ok(Command::DestroyImage {
                image: r.read_handle("DestroyImage::image")?,
            }),
            tag::DESTROY_IMAGE_VIEW_TAG => Ok(Command::DestroyImageView {
                view: r.read_handle("DestroyImageView::view")?,
            }),
            tag::DESTROY_SAMPLER_TAG => Ok(Command::DestroySampler {
                sampler: r.read_handle("DestroySampler::sampler")?,
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
                let stages = r.read_shader_stages("PushConstants::stages")?;
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
            tag::SURFACE_CAPS_TAG => Ok(Command::SurfaceCaps),
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
