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
    BlendFactor, BlendOp, BlendState, BufferBarrier, BufferCopy, BufferImageCopy, BufferUsage,
    ClearValue, ColorAttachment, ColorTargetState, ColorWrites, CompareOp, CompositeAlpha,
    CullMode, DepthBias, DepthStencilAttachment, DepthStencilState, Extent3d, FilterMode, Format,
    FrontFace, ImageAspect, ImageBarrier, ImageCopy, ImageSubresourceLayers, ImageSubresourceRange,
    ImageType, ImageUsage, ImageViewType, IndexFormat, LoadOp, MultisampleState, Offset3d,
    PassTimestampWrites, PolygonMode, PresentMode, PrimitiveState, PrimitiveTopology,
    PushConstantRange, QueueTransfer, Rect2d, ResourceState, SampleType, SamplerAddressMode,
    SemaphoreSignal, SemaphoreWait, ShaderStages, StencilFaceState, StencilOp, StencilState,
    StoreOp, Viewport,
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

    fn read_index_format(&mut self, field: &'static str) -> Result<IndexFormat, DecodeError> {
        let code = self.read_u8()?;
        tag::index_format_from_code(code).ok_or(DecodeError::InvalidEnum {
            field,
            code: code.into(),
        })
    }

    fn read_resource_state(&mut self, field: &'static str) -> Result<ResourceState, DecodeError> {
        let code = self.read_u8()?;
        tag::resource_state_from_code(code).ok_or(DecodeError::InvalidEnum {
            field,
            code: code.into(),
        })
    }

    /// A barrier's optional [`QueueTransfer`]: a presence byte, then the releasing
    /// and acquiring queue handles if present.
    fn read_queue_transfer(&mut self) -> Result<Option<QueueTransfer>, DecodeError> {
        if self.read_present("QueueTransfer")? {
            Ok(Some(QueueTransfer {
                from: self.read_handle("QueueTransfer::from")?,
                to: self.read_handle("QueueTransfer::to")?,
            }))
        } else {
            Ok(None)
        }
    }

    /// One [`BufferBarrier`]: the buffer, its `from`/`to` states, then the
    /// optional queue transfer.
    fn read_buffer_barrier(&mut self) -> Result<BufferBarrier, DecodeError> {
        Ok(BufferBarrier {
            buffer: self.read_handle("BufferBarrier::buffer")?,
            from: self.read_resource_state("BufferBarrier::from")?,
            to: self.read_resource_state("BufferBarrier::to")?,
            queue_transfer: self.read_queue_transfer()?,
        })
    }

    /// One [`ImageBarrier`]: the image, its subresource range, its `from`/`to`
    /// states, then the optional queue transfer.
    fn read_image_barrier(&mut self) -> Result<ImageBarrier, DecodeError> {
        Ok(ImageBarrier {
            image: self.read_handle("ImageBarrier::image")?,
            range: self.read_subresource_range()?,
            from: self.read_resource_state("ImageBarrier::from")?,
            to: self.read_resource_state("ImageBarrier::to")?,
            queue_transfer: self.read_queue_transfer()?,
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

    /// The presence byte, then a [`PushConstantRange`] if there is one.
    ///
    /// The optional-field rule applied to a struct rather than to a scalar or an
    /// enum — [`read_opt_compare_op`](Self::read_opt_compare_op)'s shape with a
    /// three-field body. Its `stages` reads through
    /// [`read_shader_stages`](Self::read_shader_stages), so a stage bit no flag
    /// claims is refused rather than truncated; the field crosses at all only so
    /// the replayer can refuse a `Some` by name — WebGPU has no push constants —
    /// which is why the strictness on the stages it carries still earns its
    /// place.
    fn read_opt_push_constant_range(
        &mut self,
        field: &'static str,
    ) -> Result<Option<PushConstantRange>, DecodeError> {
        if !self.read_present(field)? {
            return Ok(None);
        }
        let stages = self.read_shader_stages("PushConstantRange::stages")?;
        let offset = self.read_u32()?;
        let size = self.read_u32()?;
        Ok(Some(PushConstantRange {
            stages,
            offset,
            size,
        }))
    }

    /// The optional [`PassTimestampWrites`] both pass descriptors end with.
    ///
    /// A presence byte, then the set and the two query indices. Shared by both
    /// arms so a decode that read one of them differently is impossible rather
    /// than merely unlikely.
    fn read_timestamp_writes(
        &mut self,
        field: &'static str,
    ) -> Result<Option<PassTimestampWrites>, DecodeError> {
        if !self.read_present(field)? {
            return Ok(None);
        }
        let set = self.read_handle("PassTimestampWrites::set")?;
        let beginning_of_pass = self.read_u32()?;
        let end_of_pass = self.read_u32()?;
        Ok(Some(PassTimestampWrites {
            set,
            beginning_of_pass,
            end_of_pass,
        }))
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
                view_type: self.read_image_view_type("BindingKind::view_type")?,
                format: self.read_format("BindingKind::format")?,
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
    /// [`tag::binding_resource_code`].
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

    fn read_present_mode(&mut self, field: &'static str) -> Result<PresentMode, DecodeError> {
        let code = self.read_u8()?;
        tag::present_mode_from_code(code).ok_or(DecodeError::InvalidEnum {
            field,
            code: code.into(),
        })
    }

    fn read_composite_alpha(&mut self, field: &'static str) -> Result<CompositeAlpha, DecodeError> {
        let code = self.read_u8()?;
        tag::composite_alpha_from_code(code).ok_or(DecodeError::InvalidEnum {
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

    /// One [`ImageSubresourceLayers`] — the single mip level a copy addresses,
    /// its aspect through the same strict `from_bits` path the range's is.
    fn read_subresource_layers(&mut self) -> Result<ImageSubresourceLayers, DecodeError> {
        Ok(ImageSubresourceLayers {
            aspect: self.read_image_aspect("ImageSubresourceLayers::aspect")?,
            mip: self.read_u32()?,
            base_layer: self.read_u32()?,
            layer_count: self.read_u32()?,
        })
    }

    /// One [`Offset3d`], as three signed `i32` texel offsets in `x`, `y`, `z`
    /// order — the counterpart of [`put_offset`](crate::bytes::ByteWriter).
    fn read_offset(&mut self) -> Result<Offset3d, DecodeError> {
        Ok(Offset3d {
            x: self.read_i32()?,
            y: self.read_i32()?,
            z: self.read_i32()?,
        })
    }

    /// One [`SemaphoreWait`]: the handle, then the `u64` value.
    fn read_semaphore_wait(&mut self) -> Result<SemaphoreWait, DecodeError> {
        Ok(SemaphoreWait {
            semaphore: self.read_handle("SemaphoreWait::semaphore")?,
            value: self.read_u64()?,
        })
    }

    /// One [`SemaphoreSignal`], on [`read_semaphore_wait`](Self::read_semaphore_wait)'s
    /// terms.
    fn read_semaphore_signal(&mut self) -> Result<SemaphoreSignal, DecodeError> {
        Ok(SemaphoreSignal {
            semaphore: self.read_handle("SemaphoreSignal::semaphore")?,
            value: self.read_u64()?,
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

    fn read_compare_op(&mut self, field: &'static str) -> Result<CompareOp, DecodeError> {
        let code = self.read_u8()?;
        tag::compare_op_from_code(code).ok_or(DecodeError::InvalidEnum {
            field,
            code: code.into(),
        })
    }

    fn read_primitive_topology(
        &mut self,
        field: &'static str,
    ) -> Result<PrimitiveTopology, DecodeError> {
        let code = self.read_u8()?;
        tag::primitive_topology_from_code(code).ok_or(DecodeError::InvalidEnum {
            field,
            code: code.into(),
        })
    }

    fn read_front_face(&mut self, field: &'static str) -> Result<FrontFace, DecodeError> {
        let code = self.read_u8()?;
        tag::front_face_from_code(code).ok_or(DecodeError::InvalidEnum {
            field,
            code: code.into(),
        })
    }

    fn read_cull_mode(&mut self, field: &'static str) -> Result<CullMode, DecodeError> {
        let code = self.read_u8()?;
        tag::cull_mode_from_code(code).ok_or(DecodeError::InvalidEnum {
            field,
            code: code.into(),
        })
    }

    fn read_polygon_mode(&mut self, field: &'static str) -> Result<PolygonMode, DecodeError> {
        let code = self.read_u8()?;
        tag::polygon_mode_from_code(code).ok_or(DecodeError::InvalidEnum {
            field,
            code: code.into(),
        })
    }

    fn read_stencil_op(&mut self, field: &'static str) -> Result<StencilOp, DecodeError> {
        let code = self.read_u8()?;
        tag::stencil_op_from_code(code).ok_or(DecodeError::InvalidEnum {
            field,
            code: code.into(),
        })
    }

    fn read_blend_factor(&mut self, field: &'static str) -> Result<BlendFactor, DecodeError> {
        let code = self.read_u8()?;
        tag::blend_factor_from_code(code).ok_or(DecodeError::InvalidEnum {
            field,
            code: code.into(),
        })
    }

    fn read_blend_op(&mut self, field: &'static str) -> Result<BlendOp, DecodeError> {
        let code = self.read_u8()?;
        tag::blend_op_from_code(code).ok_or(DecodeError::InvalidEnum {
            field,
            code: code.into(),
        })
    }

    /// A [`ColorWrites`] word, through `from_bits` and never `from_bits_truncate`
    /// — the rule every bitflags field on this stream follows. A channel bit no
    /// flag claims is a build that knows a write mask this one does not, and
    /// dropping it would open a channel the caller meant closed, or close one it
    /// meant open.
    fn read_color_writes(&mut self, field: &'static str) -> Result<ColorWrites, DecodeError> {
        let bits = self.read_u32()?;
        ColorWrites::from_bits(bits).ok_or(DecodeError::InvalidEnum {
            field,
            code: bits.into(),
        })
    }

    /// One [`PrimitiveState`], in the order the struct declares its fields.
    ///
    /// Four leaf enums and a `bool`, each named for its own field so a byte read
    /// out of position is an error against the field it belongs to rather than a
    /// silently valid variant of the wrong one.
    fn read_primitive_state(&mut self) -> Result<PrimitiveState, DecodeError> {
        Ok(PrimitiveState {
            topology: self.read_primitive_topology("PrimitiveState::topology")?,
            front_face: self.read_front_face("PrimitiveState::front_face")?,
            cull_mode: self.read_cull_mode("PrimitiveState::cull_mode")?,
            polygon_mode: self.read_polygon_mode("PrimitiveState::polygon_mode")?,
            depth_clamp: self.read_bool("PrimitiveState::depth_clamp")?,
        })
    }

    /// One [`StencilFaceState`]: compare, then the three [`StencilOp`]s in the
    /// struct's order. Spelled out because the three ops are the same table three
    /// times, and any two read in the wrong order still decodes to a face state.
    fn read_stencil_face_state(&mut self) -> Result<StencilFaceState, DecodeError> {
        Ok(StencilFaceState {
            compare: self.read_compare_op("StencilFaceState::compare")?,
            fail_op: self.read_stencil_op("StencilFaceState::fail_op")?,
            depth_fail_op: self.read_stencil_op("StencilFaceState::depth_fail_op")?,
            pass_op: self.read_stencil_op("StencilFaceState::pass_op")?,
        })
    }

    /// One [`DepthStencilState`] — the deepest optional chain on the seam.
    ///
    /// The stencil is behind a presence byte, and its `front` and `back` are read
    /// in that order so a front/back swap is visible; the two masks and the
    /// three bias floats follow. **No reference:** the value a draw compares
    /// against is pass state on the seam, carried by
    /// [`Command::SetStencilReference`] and
    /// by nothing else.
    fn read_depth_stencil_state(&mut self) -> Result<DepthStencilState, DecodeError> {
        let format = self.read_format("DepthStencilState::format")?;
        let depth_write = self.read_bool("DepthStencilState::depth_write")?;
        let depth_compare = self.read_compare_op("DepthStencilState::depth_compare")?;
        let stencil = if self.read_present("DepthStencilState::stencil")? {
            let front = self.read_stencil_face_state()?;
            let back = self.read_stencil_face_state()?;
            let read_mask = self.read_u32()?;
            let write_mask = self.read_u32()?;
            Some(StencilState {
                front,
                back,
                read_mask,
                write_mask,
            })
        } else {
            None
        };
        let bias = DepthBias {
            constant: self.read_f32()?,
            slope_scale: self.read_f32()?,
            clamp: self.read_f32()?,
        };
        Ok(DepthStencilState {
            format,
            depth_write,
            depth_compare,
            stencil,
            bias,
        })
    }

    /// One [`MultisampleState`]: the sample count, then the coverage flag, with
    /// no sample-mask word between them — the seam has none, so neither does the
    /// wire. See [`STREAM_VERSION`](tag::STREAM_VERSION) for why removing it is
    /// a version bump.
    fn read_multisample_state(&mut self) -> Result<MultisampleState, DecodeError> {
        Ok(MultisampleState {
            samples: self.read_u32()?,
            alpha_to_coverage: self.read_bool("MultisampleState::alpha_to_coverage")?,
        })
    }

    /// One [`BlendState`]: colour source/dest/op, then alpha source/dest/op, in
    /// the struct's order — six leaf codes any two of which read in the wrong
    /// order still decodes to a blend state.
    fn read_blend_state(&mut self) -> Result<BlendState, DecodeError> {
        Ok(BlendState {
            color_src: self.read_blend_factor("BlendState::color_src")?,
            color_dst: self.read_blend_factor("BlendState::color_dst")?,
            color_op: self.read_blend_op("BlendState::color_op")?,
            alpha_src: self.read_blend_factor("BlendState::alpha_src")?,
            alpha_dst: self.read_blend_factor("BlendState::alpha_dst")?,
            alpha_op: self.read_blend_op("BlendState::alpha_op")?,
        })
    }

    /// One [`ColorTargetState`]: format, an optional blend behind a presence
    /// byte, and the [`ColorWrites`] mask.
    fn read_color_target_state(&mut self) -> Result<ColorTargetState, DecodeError> {
        let format = self.read_format("ColorTargetState::format")?;
        let blend = if self.read_present("ColorTargetState::blend")? {
            Some(self.read_blend_state()?)
        } else {
            None
        };
        let write_mask = self.read_color_writes("ColorTargetState::write_mask")?;
        Ok(ColorTargetState {
            format,
            blend,
            write_mask,
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
            tag::CREATE_OFFSCREEN_SURFACE_TAG => {
                // Only the handle: no canvas key, no extent, no format — see
                // `Command::CreateOffscreenSurface`.
                let surface = r.read_handle("CreateOffscreenSurface::surface")?;
                Ok(Command::CreateOffscreenSurface { surface })
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
            tag::CREATE_PIPELINE_LAYOUT_TAG => {
                let layout = r.read_handle("CreatePipelineLayout::layout")?;
                let label = r.read_opt_string("PipelineLayoutDesc::label")?;
                let count = r.read_count("PipelineLayoutDesc::bind_group_layouts")?;
                // Pushed in wire order and never sorted: set order is what a
                // shader's `@group(n)` indexes, so the slice's order is part of
                // the value. See `Command::CreatePipelineLayout`.
                let mut bind_group_layouts = Vec::with_capacity(count);
                for _ in 0..count {
                    bind_group_layouts
                        .push(r.read_handle("PipelineLayoutDesc::bind_group_layouts")?);
                }
                let push_constants =
                    r.read_opt_push_constant_range("PipelineLayoutDesc::push_constants")?;
                Ok(Command::CreatePipelineLayout {
                    layout,
                    label,
                    bind_group_layouts,
                    push_constants,
                })
            }
            tag::CREATE_COMPUTE_PIPELINE_TAG => {
                // Two handles into two *different* tables — `layout` into the
                // pipeline-layout table and `module` into the shader-module table
                // — so they are read one at a time and named for the field each
                // fills, since a handle carries no kind and the two could hold
                // identical bits. `workgroup_size` crosses whole though the WebGPU
                // replayer drops it; see `Command::CreateComputePipeline`.
                let pipeline = r.read_handle("CreateComputePipeline::pipeline")?;
                let label = r.read_opt_string("ComputePipelineDesc::label")?;
                let layout = r.read_handle("ComputePipelineDesc::layout")?;
                let module = r.read_handle("ShaderEntry::module")?;
                let entry_point = r.read_string("ShaderEntry::entry_point")?;
                let workgroup_size = [r.read_u32()?, r.read_u32()?, r.read_u32()?];
                Ok(Command::CreateComputePipeline {
                    pipeline,
                    label,
                    layout,
                    module,
                    entry_point,
                    workgroup_size,
                })
            }
            tag::CREATE_GRAPHICS_PIPELINE_TAG => {
                // The largest descriptor on the seam. The vertex stage is a
                // module and an entry point and no buffer layout — vertex pulling
                // — and the fragment stage rides a presence byte, `None` for a
                // depth-only pass. The four state blocks that follow are read
                // through the field readers above, deepest of them the
                // depth-stencil chain. Nothing is validated: `workgroup_size`'s
                // rule holds here for the whole tree, and every refusal is the
                // replayer's. See `Command::CreateGraphicsPipeline`.
                let pipeline = r.read_handle("CreateGraphicsPipeline::pipeline")?;
                let label = r.read_opt_string("GraphicsPipelineDesc::label")?;
                let layout = r.read_handle("GraphicsPipelineDesc::layout")?;
                let vertex_module = r.read_handle("ShaderEntry::module")?;
                let vertex_entry_point = r.read_string("ShaderEntry::entry_point")?;
                let fragment = if r.read_present("GraphicsPipelineDesc::fragment")? {
                    let module = r.read_handle("ShaderEntry::module")?;
                    let entry_point = r.read_string("ShaderEntry::entry_point")?;
                    Some((module, entry_point))
                } else {
                    None
                };
                let primitive = r.read_primitive_state()?;
                let depth_stencil = if r.read_present("GraphicsPipelineDesc::depth_stencil")? {
                    Some(r.read_depth_stencil_state()?)
                } else {
                    None
                };
                let multisample = r.read_multisample_state()?;
                let count = r.read_count("GraphicsPipelineDesc::color_targets")?;
                let mut color_targets = Vec::with_capacity(count);
                for _ in 0..count {
                    color_targets.push(r.read_color_target_state()?);
                }
                Ok(Command::CreateGraphicsPipeline {
                    pipeline,
                    label,
                    layout,
                    vertex_module,
                    vertex_entry_point,
                    fragment,
                    primitive,
                    depth_stencil,
                    multisample,
                    color_targets,
                })
            }
            tag::CREATE_QUERY_SET_TAG => {
                // The handle, the label, the kind code, then the count. See
                // `Command::CreateQuerySet`.
                let set = r.read_handle("CreateQuerySet::set")?;
                let label = r.read_opt_string("QuerySetDesc::label")?;
                let kind_code = r.read_u8()?;
                let kind =
                    tag::query_kind_from_code(kind_code).ok_or(DecodeError::InvalidEnum {
                        field: "QuerySetDesc::kind",
                        code: kind_code.into(),
                    })?;
                let count = r.read_u32()?;
                Ok(Command::CreateQuerySet {
                    set,
                    label,
                    kind,
                    count,
                })
            }
            tag::DESTROY_COMPUTE_PIPELINE_TAG => Ok(Command::DestroyComputePipeline {
                pipeline: r.read_handle("DestroyComputePipeline::pipeline")?,
            }),
            tag::DESTROY_GRAPHICS_PIPELINE_TAG => Ok(Command::DestroyGraphicsPipeline {
                pipeline: r.read_handle("DestroyGraphicsPipeline::pipeline")?,
            }),
            tag::DESTROY_SHADER_MODULE_TAG => Ok(Command::DestroyShaderModule {
                module: r.read_handle("DestroyShaderModule::module")?,
            }),
            tag::DESTROY_PIPELINE_LAYOUT_TAG => Ok(Command::DestroyPipelineLayout {
                layout: r.read_handle("DestroyPipelineLayout::layout")?,
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
            tag::DESTROY_QUERY_SET_TAG => Ok(Command::DestroyQuerySet {
                set: r.read_handle("DestroyQuerySet::set")?,
            }),
            tag::BEGIN_DEBUG_LABEL_TAG => Ok(Command::BeginDebugLabel {
                label: r.read_string("BeginDebugLabel::label")?,
            }),
            tag::END_DEBUG_LABEL_TAG => Ok(Command::EndDebugLabel),
            tag::INSERT_DEBUG_MARKER_TAG => Ok(Command::InsertDebugMarker {
                label: r.read_string("InsertDebugMarker::label")?,
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
                let timestamp_writes =
                    r.read_timestamp_writes("RenderPassDesc::timestamp_writes")?;
                Ok(Command::BeginRenderPass {
                    label,
                    color_attachments,
                    depth_stencil_attachment,
                    render_area,
                    timestamp_writes,
                })
            }
            tag::BIND_GRAPHICS_PIPELINE_TAG => Ok(Command::BindGraphicsPipeline {
                pipeline: r.read_handle("BindGraphicsPipeline::pipeline")?,
            }),
            tag::SET_VIEWPORT_TAG => {
                // `Viewport` in declaration order: the rectangle's four floats,
                // then the depth range's two. See `Command::SetViewport`.
                let x = r.read_f32()?;
                let y = r.read_f32()?;
                let width = r.read_f32()?;
                let height = r.read_f32()?;
                let depth_min = r.read_f32()?;
                let depth_max = r.read_f32()?;
                Ok(Command::SetViewport {
                    viewport: Viewport {
                        x,
                        y,
                        width,
                        height,
                        depth_min,
                        depth_max,
                    },
                })
            }
            tag::SET_SCISSOR_TAG => Ok(Command::SetScissor {
                rect: r.read_rect()?,
            }),
            tag::SET_STENCIL_REFERENCE_TAG => Ok(Command::SetStencilReference {
                reference: r.read_u32()?,
            }),
            tag::BIND_INDEX_BUFFER_TAG => {
                // The buffer, its byte offset, then the index-format code. See
                // `Command::BindIndexBuffer`.
                let buffer = r.read_handle("BindIndexBuffer::buffer")?;
                let offset = r.read_u64()?;
                let format = r.read_index_format("BindIndexBuffer::format")?;
                Ok(Command::BindIndexBuffer {
                    buffer,
                    offset,
                    format,
                })
            }
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
            tag::DRAW_INDEXED_TAG => {
                // The index range's start and end, the signed `base_vertex`, then
                // the instance range's start and end — spelled out for `DRAW_TAG`'s
                // reason. See `Command::DrawIndexed`.
                let first_index = r.read_u32()?;
                let last_index = r.read_u32()?;
                let base_vertex = r.read_i32()?;
                let first_instance = r.read_u32()?;
                let last_instance = r.read_u32()?;
                Ok(Command::DrawIndexed {
                    indices: first_index..last_index,
                    base_vertex,
                    instances: first_instance..last_instance,
                })
            }
            tag::DRAW_INDIRECT_TAG => {
                // The argument buffer, its byte offset, the CPU-known draw count,
                // then the stride. See `Command::DrawIndirect`.
                let buffer = r.read_handle("DrawIndirect::buffer")?;
                let offset = r.read_u64()?;
                let draw_count = r.read_u32()?;
                let stride = r.read_u32()?;
                Ok(Command::DrawIndirect {
                    buffer,
                    offset,
                    draw_count,
                    stride,
                })
            }
            tag::DRAW_INDEXED_INDIRECT_TAG => {
                // The same four fields `DRAW_INDIRECT_TAG` carries, in the same
                // order. See `Command::DrawIndexedIndirect`.
                let buffer = r.read_handle("DrawIndexedIndirect::buffer")?;
                let offset = r.read_u64()?;
                let draw_count = r.read_u32()?;
                let stride = r.read_u32()?;
                Ok(Command::DrawIndexedIndirect {
                    buffer,
                    offset,
                    draw_count,
                    stride,
                })
            }
            tag::BEGIN_COMPUTE_PASS_TAG => {
                let label = r.read_opt_string("ComputePassDesc::label")?;
                let timestamp_writes =
                    r.read_timestamp_writes("ComputePassDesc::timestamp_writes")?;
                Ok(Command::BeginComputePass {
                    label,
                    timestamp_writes,
                })
            }
            tag::BIND_COMPUTE_PIPELINE_TAG => Ok(Command::BindComputePipeline {
                pipeline: r.read_handle("BindComputePipeline::pipeline")?,
            }),
            tag::DISPATCH_TAG => {
                let x = r.read_u32()?;
                let y = r.read_u32()?;
                let z = r.read_u32()?;
                Ok(Command::Dispatch { x, y, z })
            }
            tag::DISPATCH_INDIRECT_TAG => {
                // The argument buffer, then its byte offset. No count and no
                // stride — see `Command::DispatchIndirect`.
                let buffer = r.read_handle("DispatchIndirect::buffer")?;
                let offset = r.read_u64()?;
                Ok(Command::DispatchIndirect { buffer, offset })
            }
            tag::END_COMPUTE_PASS_TAG => Ok(Command::EndComputePass),
            tag::RESET_QUERY_SET_TAG => {
                // The set, then the range as a first index and a count. See
                // `Command::ResetQuerySet`.
                let set = r.read_handle("ResetQuerySet::set")?;
                let first_query = r.read_u32()?;
                let query_count = r.read_u32()?;
                Ok(Command::ResetQuerySet {
                    set,
                    first_query,
                    query_count,
                })
            }
            tag::RESOLVE_QUERY_SET_TAG => {
                // The set and its range, then the destination and its byte
                // offset. See `Command::ResolveQuerySet`.
                let set = r.read_handle("ResolveQuerySet::set")?;
                let first_query = r.read_u32()?;
                let query_count = r.read_u32()?;
                let dst = r.read_handle("ResolveQuerySet::dst")?;
                let dst_offset = r.read_u64()?;
                Ok(Command::ResolveQuerySet {
                    set,
                    first_query,
                    query_count,
                    dst,
                    dst_offset,
                })
            }
            tag::QUERY_RESULTS_TAG => {
                // The set and the range to read. See `Command::QueryResults`.
                let set = r.read_handle("QueryResults::set")?;
                let first_query = r.read_u32()?;
                let query_count = r.read_u32()?;
                Ok(Command::QueryResults {
                    set,
                    first_query,
                    query_count,
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
            tag::CREATE_SWAPCHAIN_TAG => {
                // `SwapchainDesc` in declaration order behind the caller-allocated
                // handle: the surface, the format, the extent's two components, the
                // image count, then the present-mode and composite-alpha enum
                // codes. `image_count` and `present_mode` are carried verbatim
                // even though the replayer drops them — see `Command::CreateSwapchain`.
                let swapchain = r.read_handle("CreateSwapchain::swapchain")?;
                let label = r.read_opt_string("SwapchainDesc::label")?;
                let surface = r.read_handle("SwapchainDesc::surface")?;
                let format = r.read_format("SwapchainDesc::format")?;
                let width = r.read_u32()?;
                let height = r.read_u32()?;
                let image_count = r.read_u32()?;
                let present_mode = r.read_present_mode("SwapchainDesc::present_mode")?;
                let composite_alpha = r.read_composite_alpha("SwapchainDesc::composite_alpha")?;
                Ok(Command::CreateSwapchain {
                    swapchain,
                    label,
                    surface,
                    format,
                    extent: (width, height),
                    image_count,
                    present_mode,
                    composite_alpha,
                })
            }
            tag::ACQUIRE_NEXT_FRAME_TAG => {
                // The swapchain, then the two caller-allocated handles the acquired
                // texture and its view are filed under — three handles that mean
                // different things, so spelled out one at a time.
                let swapchain = r.read_handle("AcquireNextFrame::swapchain")?;
                let image = r.read_handle("AcquireNextFrame::image")?;
                let view = r.read_handle("AcquireNextFrame::view")?;
                Ok(Command::AcquireNextFrame {
                    swapchain,
                    image,
                    view,
                })
            }
            tag::PRESENT_TAG => {
                // The swapchain, the counted waits, then the optional `present_id`
                // behind a presence byte. The wait list is decoded whole so the
                // replayer can refuse a non-empty one by name. See `Command::Present`.
                let swapchain = r.read_handle("PresentInfo::swapchain")?;
                let wait_count = r.read_count("PresentInfo::waits")?;
                let mut waits = Vec::with_capacity(wait_count);
                for _ in 0..wait_count {
                    waits.push(r.read_handle("PresentInfo::waits")?);
                }
                let present_id = if r.read_present("PresentInfo::present_id")? {
                    Some(r.read_u64()?)
                } else {
                    None
                };
                Ok(Command::Present {
                    swapchain,
                    waits,
                    present_id,
                })
            }
            tag::DESTROY_SWAPCHAIN_TAG => Ok(Command::DestroySwapchain {
                swapchain: r.read_handle("DestroySwapchain::swapchain")?,
            }),
            tag::RECONFIGURE_SWAPCHAIN_TAG => {
                // The same `SwapchainDesc` layout `CreateSwapchain` decodes, behind
                // the handle of the swapchain to re-configure. Parallel to that arm
                // rather than sharing a helper: the two build different variants and
                // the round-trip corpus is what proves they stay in step — see
                // `Command::ReconfigureSwapchain`.
                let swapchain = r.read_handle("ReconfigureSwapchain::swapchain")?;
                let label = r.read_opt_string("SwapchainDesc::label")?;
                let surface = r.read_handle("SwapchainDesc::surface")?;
                let format = r.read_format("SwapchainDesc::format")?;
                let width = r.read_u32()?;
                let height = r.read_u32()?;
                let image_count = r.read_u32()?;
                let present_mode = r.read_present_mode("SwapchainDesc::present_mode")?;
                let composite_alpha = r.read_composite_alpha("SwapchainDesc::composite_alpha")?;
                Ok(Command::ReconfigureSwapchain {
                    swapchain,
                    label,
                    surface,
                    format,
                    extent: (width, height),
                    image_count,
                    present_mode,
                    composite_alpha,
                })
            }
            tag::CREATE_COMMAND_ENCODER_TAG => {
                // No handle: the encoder is the replayer's implicit-current one,
                // as `crcbl-hal`'s recording methods assume no receiver. `queue`
                // crosses and selects nothing — see `Command::CreateCommandEncoder`.
                let label = r.read_opt_string("CommandEncoderDesc::label")?;
                let queue = r.read_handle("CommandEncoderDesc::queue")?;
                Ok(Command::CreateCommandEncoder { label, queue })
            }
            tag::END_RENDER_PASS_TAG => Ok(Command::EndRenderPass),
            tag::COPY_IMAGE_TO_BUFFER_TAG => {
                // `BufferImageCopy` in declaration order; the direction is the
                // opcode, never a field. `buffer_row_length` is texels and `0` is
                // tightly packed — both cross verbatim, the byte conversion the
                // replayer's. See `Command::CopyImageToBuffer`.
                let buffer = r.read_handle("BufferImageCopy::buffer")?;
                let buffer_offset = r.read_u64()?;
                let buffer_row_length = r.read_u32()?;
                let buffer_image_height = r.read_u32()?;
                let image = r.read_handle("BufferImageCopy::image")?;
                let image_subresource = r.read_subresource_layers()?;
                let image_offset = r.read_offset()?;
                let image_extent = r.read_extent()?;
                Ok(Command::CopyImageToBuffer {
                    buffer,
                    buffer_offset,
                    buffer_row_length,
                    buffer_image_height,
                    image,
                    image_subresource,
                    image_offset,
                    image_extent,
                })
            }
            tag::COPY_BUFFER_TO_BUFFER_TAG => {
                // `BufferCopy` in declaration order: source and its offset,
                // destination and its offset, then the size.
                let src = r.read_handle("BufferCopy::src")?;
                let src_offset = r.read_u64()?;
                let dst = r.read_handle("BufferCopy::dst")?;
                let dst_offset = r.read_u64()?;
                let size = r.read_u64()?;
                Ok(Command::CopyBufferToBuffer {
                    copy: BufferCopy {
                        src,
                        src_offset,
                        dst,
                        dst_offset,
                        size,
                    },
                })
            }
            tag::COPY_BUFFER_TO_IMAGE_TAG => {
                // `BufferImageCopy` in declaration order, the same layout
                // `CopyImageToBuffer` reads; the direction is the opcode, never a
                // field. See `Command::CopyBufferToImage`.
                let buffer = r.read_handle("BufferImageCopy::buffer")?;
                let buffer_offset = r.read_u64()?;
                let buffer_row_length = r.read_u32()?;
                let buffer_image_height = r.read_u32()?;
                let image = r.read_handle("BufferImageCopy::image")?;
                let image_subresource = r.read_subresource_layers()?;
                let image_offset = r.read_offset()?;
                let image_extent = r.read_extent()?;
                Ok(Command::CopyBufferToImage {
                    copy: BufferImageCopy {
                        buffer,
                        buffer_offset,
                        buffer_row_length,
                        buffer_image_height,
                        image,
                        image_subresource,
                        image_offset,
                        image_extent,
                    },
                })
            }
            tag::COPY_IMAGE_TO_IMAGE_TAG => {
                // `ImageCopy` in declaration order: source, its subresource and
                // offset, destination, its subresource and offset, then the extent.
                let src = r.read_handle("ImageCopy::src")?;
                let src_subresource = r.read_subresource_layers()?;
                let src_offset = r.read_offset()?;
                let dst = r.read_handle("ImageCopy::dst")?;
                let dst_subresource = r.read_subresource_layers()?;
                let dst_offset = r.read_offset()?;
                let extent = r.read_extent()?;
                Ok(Command::CopyImageToImage {
                    copy: ImageCopy {
                        src,
                        src_subresource,
                        src_offset,
                        dst,
                        dst_subresource,
                        dst_offset,
                        extent,
                    },
                })
            }
            tag::CLEAR_BUFFER_TAG => {
                let buffer = r.read_handle("ClearBuffer::buffer")?;
                let offset = r.read_u64()?;
                let size = r.read_u64()?;
                Ok(Command::ClearBuffer {
                    buffer,
                    offset,
                    size,
                })
            }
            tag::WRITE_BUFFER_TAG => {
                // A host→buffer upload: the buffer, its byte offset, then the
                // bytes. The payload is owned, as `PushConstants::data` is. See
                // `Command::WriteBuffer`.
                let buffer = r.read_handle("WriteBuffer::buffer")?;
                let offset = r.read_u64()?;
                let data = r.read_field("WriteBuffer::data")?.to_vec();
                Ok(Command::WriteBuffer {
                    buffer,
                    offset,
                    data,
                })
            }
            tag::PIPELINE_BARRIER_TAG => {
                // The `Barriers` batch: the counted buffer list, the counted
                // image list, then the `global` flag. Symmetric with
                // `StreamWriter::pipeline_barrier`; the replayer does nothing with
                // it, but it decodes whole so it round-trips. See
                // `Command::PipelineBarrier`.
                let buffer_count = r.read_count("Barriers::buffers")?;
                let mut buffers = Vec::with_capacity(buffer_count);
                for _ in 0..buffer_count {
                    buffers.push(r.read_buffer_barrier()?);
                }
                let image_count = r.read_count("Barriers::images")?;
                let mut images = Vec::with_capacity(image_count);
                for _ in 0..image_count {
                    images.push(r.read_image_barrier()?);
                }
                let global = r.read_bool("Barriers::global")?;
                Ok(Command::PipelineBarrier {
                    buffers,
                    images,
                    global,
                })
            }
            tag::FINISH_TAG => Ok(Command::Finish {
                command_buffer: r.read_handle("Finish::command_buffer")?,
            }),
            tag::SUBMIT_TAG => {
                let count = r.read_count("SubmitInfo::command_buffers")?;
                let mut command_buffers = Vec::with_capacity(count);
                for _ in 0..count {
                    command_buffers.push(r.read_handle("SubmitInfo::command_buffers")?);
                }
                let wait_count = r.read_count("SubmitInfo::waits")?;
                let mut waits = Vec::with_capacity(wait_count);
                for _ in 0..wait_count {
                    waits.push(r.read_semaphore_wait()?);
                }
                let signal_count = r.read_count("SubmitInfo::signals")?;
                let mut signals = Vec::with_capacity(signal_count);
                for _ in 0..signal_count {
                    signals.push(r.read_semaphore_signal()?);
                }
                Ok(Command::Submit {
                    command_buffers,
                    waits,
                    signals,
                })
            }
            tag::REQUEST_READBACK_TAG => {
                let readback = r.read_handle("RequestReadback::readback")?;
                let label = r.read_opt_string("ReadbackDesc::label")?;
                let buffer = r.read_handle("ReadbackDesc::buffer")?;
                let offset = r.read_u64()?;
                let size = r.read_u64()?;
                // A presence byte then the wait, as `create_sampler`'s `compare`
                // is: `None` is `mapAsync`, `Some` the semaphore wait the replayer
                // refuses. See `Command::RequestReadback`.
                let after = if r.read_present("ReadbackDesc::after")? {
                    Some(r.read_semaphore_wait()?)
                } else {
                    None
                };
                Ok(Command::RequestReadback {
                    readback,
                    label,
                    buffer,
                    offset,
                    size,
                    after,
                })
            }
            tag::POLL_READBACK_TAG => Ok(Command::PollReadback {
                readback: r.read_handle("PollReadback::readback")?,
            }),
            // No body: the HAL call takes nothing, and this seam holds one
            // device. A decoder that read a field here would consume whatever
            // follows, which is why the corpus puts a command after it.
            tag::TAKE_ERROR_TAG => Ok(Command::TakeError),
            tag::DESTROY_READBACK_TAG => Ok(Command::DestroyReadback {
                readback: r.read_handle("DestroyReadback::readback")?,
            }),
            tag::DESTROY_COMMAND_BUFFER_TAG => Ok(Command::DestroyCommandBuffer {
                command_buffer: r.read_handle("DestroyCommandBuffer::command_buffer")?,
            }),
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
