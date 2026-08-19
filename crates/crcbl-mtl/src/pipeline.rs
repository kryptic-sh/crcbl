//! Shader modules, pipeline layouts and pipeline state — the rung that turns a
//! clear into a triangle.
//!
//! # A shader module is an `MTLLibrary` compiled from MSL *source*
//!
//! `crcbl-shaders` commits `msl/*.metal`, and
//! [`create_shader_module`](crcbl_hal::Device::create_shader_module) hands it
//! straight to `MTLDevice::newLibraryWithSource:options:error:`. There is no
//! offline step: Apple's `metal` compiler ships only with Xcode, so a
//! pre-linked `.metallib` could not be produced by the artifact script that
//! every leg of CI runs. The crate-level docs of `crcbl-shaders` argue that in
//! full.
//!
//! **The `NSError` is not swallowed.** A module that fails to compile produces
//! [`HalError::ShaderCompilation`] carrying Metal's own diagnostic — file, line
//! and message — because that text is the only debugging aid a caller gets, and
//! a generic "shader compilation failed" would send them to read an artifact
//! they cannot see the compiler's opinion of.
//!
//! Entry points are matched **by name**: `newFunctionWithName:` is a lookup in
//! the compiled library, and the names in the MSL are the ones
//! `spirv/manifest.txt` recorded from the SPIR-V's `OpEntryPoint`. That
//! agreement is not assumed here — `crcbl-shaders`' own
//! `every_shipped_shader_has_msl_naming_the_same_entry_points` checks it with
//! no Mac in the loop — but a mismatch still surfaces here, by name, rather
//! than as a nil function pointer reaching the pipeline descriptor.
//!
//! # A pipeline layout is where the argument tables are laid out
//!
//! Metal has no pipeline-layout object, so [`PipelineLayoutEntry`] holds the
//! only thing a layout is good for here: **where each set starts in Metal's
//! flat argument tables**. `crcbl_mtl::binding` owns that rule and the evidence
//! behind it; this module is where it is applied, because
//! [`create_pipeline_layout`](crcbl_hal::Device::create_pipeline_layout) is the
//! call that sees every set at once.
//!
//! The empty layout is still legal and still useful: a pipeline whose shaders
//! read no resources needs no bindings at all, which is what makes the
//! hand-written triangle in `device.rs`'s tests a real draw through the real
//! seam.
//!
//! A **push-constant range is one more buffer argument**, because that is what
//! Slang lowers it to: [`crate::argument`] decides the index it takes and the
//! bytes it spans, and `crate::command`'s `push_constants` feeds it with
//! `setBytes:length:atIndex:`. See that module on why the index is the one
//! *after* every binding rather than a number chosen here, and on why the whole
//! block is re-sent on every write.
//!
//! There is no Metal object for it either, so a layout entry carries the
//! [`Block`](crate::argument::Block) itself: the index and the length *are* the
//! binding.
//!
//! # Metal splits pipeline state in two, so a pipeline entry carries both
//!
//! An `MTLRenderPipelineState` is only *half* of what
//! [`GraphicsPipelineDesc`](crcbl_hal::GraphicsPipelineDesc) describes. Metal
//! puts the shader functions, attachment formats, blend state and sample count
//! in the pipeline object, and leaves the rasteriser knobs — cull mode,
//! winding, fill mode, depth clip, depth bias — plus the depth/stencil test
//! itself on the **encoder**, as calls made between draws.
//!
//! Vulkan bakes all of it into one `VkPipeline`, and the seam is Vulkan-shaped,
//! so this backend stores the encoder half in [`RasterState`] beside the
//! pipeline object and replays it in
//! [`bind_graphics_pipeline`](crcbl_hal::CommandEncoder::bind_graphics_pipeline).
//! The result is that binding a pipeline sets *everything* the descriptor asked
//! for, which is what the seam promises — rather than the subset Metal happens
//! to keep in a pipeline object.
//!
//! The primitive topology is in that half too, and one step further out:
//! Metal takes it at the **draw call**. So [`RasterState::primitive`] survives
//! until `draw` reads it, and a draw with nothing bound is refused rather than
//! guessing at triangles.
//!
//! # Reversed-Z crosses unchanged
//!
//! [`CompareOp::Greater`](crcbl_hal::CompareOp) reaches
//! `MTLCompareFunction::Greater` through `conv::compare_function`, and the
//! depth range on the viewport is left alone. Both are the engine's locked
//! convention (`crcbl_hal::depth`), produced by the projection matrix above
//! this seam; a backend that "corrected" either would apply it twice.

use crcbl_hal::{
    ComputePipelineDesc, ComputePipelineHandle, DepthStencilState, GraphicsPipelineDesc,
    GraphicsPipelineHandle, HalError, MultisampleState, PipelineLayoutDesc, PipelineLayoutHandle,
    ShaderEntry, ShaderModuleDesc, ShaderModuleHandle, ShaderSources, StencilState,
};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::NSString;
use objc2_metal::{
    MTLCompareFunction, MTLComputePipelineDescriptor, MTLComputePipelineState, MTLCullMode,
    MTLDepthClipMode, MTLDepthStencilDescriptor, MTLDepthStencilState, MTLDevice, MTLFunction,
    MTLLibrary, MTLPipelineOption, MTLPrimitiveType, MTLRenderPipelineDescriptor,
    MTLRenderPipelineState, MTLSize, MTLStencilDescriptor, MTLStencilOperation,
    MTLTriangleFillMode, MTLWinding,
};

use crate::conv;
use crate::device::{DeviceInner, MetalDevice, Owned, lookup, owned, to_ns};

/// A compiled `MTLLibrary`, and the functions a pipeline pulls out of it.
#[derive(Debug)]
pub(crate) struct ShaderModuleEntry {
    pub(crate) owner: u64,
    pub(crate) raw: Retained<ProtocolObject<dyn MTLLibrary>>,
}

/// A pipeline layout: where each of its sets starts in Metal's argument tables.
///
/// There is no Metal object behind it — no descriptor set layout to combine and
/// no root signature to build — so what it holds is the *flattening*
/// `crcbl_mtl::binding` computes: the per-table base index each set's bindings
/// are offset by. That is the whole reason
/// [`bind_group`](crcbl_hal::CommandEncoder::bind_group) takes a pipeline
/// layout at all, and it is why the empty layout is still a pool entry rather
/// than a synthesised handle: obligation 3 requires a pipeline naming another
/// device's layout to be caught as [`HalError::ForeignObject`], which a handle
/// nobody allocated could not provide.
#[derive(Debug)]
pub(crate) struct PipelineLayoutEntry {
    pub(crate) owner: u64,
    /// In set order, so `sets[n]` is what `bind_group(n, …)` binds into.
    pub(crate) sets: Vec<crate::binding::SetPlacement>,
    /// The push-constant block, if the descriptor declared a range: which
    /// buffer-table entry it occupies, how many bytes it spans and which stages
    /// read it. `None` is a layout with no range, and a `push_constants` call
    /// naming one is refused rather than dropped.
    pub(crate) push_constants: Option<crate::argument::Block>,
}

/// The half of a graphics pipeline Metal keeps on the encoder rather than in
/// the pipeline object. See the module docs.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RasterState {
    /// What `drawPrimitives:` is told to assemble. Metal takes this per draw,
    /// so it lives here until the draw reads it.
    pub(crate) primitive: MTLPrimitiveType,
    pub(crate) cull: MTLCullMode,
    pub(crate) winding: MTLWinding,
    pub(crate) fill: MTLTriangleFillMode,
    pub(crate) clip: MTLDepthClipMode,
    /// `setDepthBias:slopeScale:clamp:`, in that order. Zeroes when the
    /// pipeline has no depth state, which is what Metal's own default is.
    pub(crate) bias: [f32; 3],
}

/// A graphics pipeline: the Metal object, plus the state Metal would not take.
#[derive(Debug)]
pub(crate) struct GraphicsPipelineEntry {
    pub(crate) owner: u64,
    pub(crate) raw: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    /// Never absent. A pipeline that declares no depth/stencil state carries
    /// the device's [`DeviceInner::default_depth_stencil`] instead, which is
    /// what keeps `setDepthStencilState:` from ever being handed nil — see
    /// [`default_depth_stencil_state`].
    pub(crate) depth_stencil: Retained<ProtocolObject<dyn MTLDepthStencilState>>,
    pub(crate) raster: RasterState,
}

/// A compute pipeline, and the threadgroup size Metal will not take until the
/// dispatch.
#[derive(Debug)]
pub(crate) struct ComputePipelineEntry {
    pub(crate) owner: u64,
    pub(crate) raw: Retained<ProtocolObject<dyn MTLComputePipelineState>>,
    /// [`ComputePipelineDesc::workgroup_size`](crcbl_hal::ComputePipelineDesc::workgroup_size),
    /// as the `MTLSize` every `dispatchThreadgroups:threadsPerThreadgroup:` on
    /// this pipeline passes.
    ///
    /// Carried here rather than re-derived at the dispatch because the dispatch
    /// call has only the *bound* pipeline to ask, and Metal's encoder does not
    /// hand its pipeline state back.
    pub(crate) threads_per_threadgroup: MTLSize,
}

owned!(
    ShaderModuleEntry,
    PipelineLayoutEntry,
    GraphicsPipelineEntry,
    ComputePipelineEntry,
);

/// A graphics pipeline resolved to everything the render encoder must be told.
///
/// Cloned out from under the device lock, exactly as
/// [`DeviceInner::buffer_raw`] does and for the same reason: the encoder
/// resolves handles with the lock held and then encodes without it.
pub(crate) struct BoundPipeline {
    pub(crate) raw: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    /// Never absent, as [`GraphicsPipelineEntry::depth_stencil`] says.
    pub(crate) depth_stencil: Retained<ProtocolObject<dyn MTLDepthStencilState>>,
    pub(crate) raster: RasterState,
}

impl DeviceInner {
    /// The pipeline a handle names, with the encoder-side state beside it.
    pub(crate) fn graphics_pipeline_raw(
        &self,
        handle: GraphicsPipelineHandle,
    ) -> Result<BoundPipeline, HalError> {
        let state = self.state();
        let entry = lookup(&state.graphics_pipelines, "graphics pipeline", handle, self)?;
        Ok(BoundPipeline {
            raw: entry.raw.clone(),
            depth_stencil: entry.depth_stencil.clone(),
            raster: entry.raster,
        })
    }

    /// The compute pipeline a handle names, with the threadgroup size the
    /// dispatch after it will need.
    pub(crate) fn compute_pipeline_raw(
        &self,
        handle: ComputePipelineHandle,
    ) -> Result<
        (
            Retained<ProtocolObject<dyn MTLComputePipelineState>>,
            MTLSize,
        ),
        HalError,
    > {
        let state = self.state();
        let entry = lookup(&state.compute_pipelines, "compute pipeline", handle, self)?;
        Ok((entry.raw.clone(), entry.threads_per_threadgroup))
    }

    /// The push-constant block a pipeline layout declares.
    ///
    /// Resolved with the device lock held and applied without it, exactly as
    /// [`bind_group_raw`](DeviceInner::bind_group_raw) is and for the same
    /// reason: the encoder reads device state before it records, never during.
    ///
    /// # Errors
    ///
    /// As [`lookup`] for the layout, plus [`HalError::InvalidDescriptor`] when
    /// the layout declares no range at all — the seam requires a write to fail
    /// loudly rather than be dropped, and a caller here is holding a layout it
    /// did not declare push constants on.
    pub(crate) fn push_constant_block(
        &self,
        layout: PipelineLayoutHandle,
    ) -> Result<crate::argument::Block, HalError> {
        let state = self.state();
        let entry = lookup(&state.pipeline_layouts, "pipeline layout", layout, self)?;
        entry.push_constants.ok_or_else(|| {
            HalError::InvalidDescriptor(
                "push_constants through a pipeline layout that declares no push-constant range: \
                 the block is a buffer argument this backend places from the range, so there is \
                 no argument-table index to write it at"
                    .to_string(),
            )
        })
    }
}

impl MetalDevice {
    /// Compiles MSL into an `MTLLibrary`.
    ///
    /// See the module docs for why the artifact is source, and why Metal's own
    /// error text is carried through verbatim.
    pub(crate) fn create_shader_module_impl(
        &self,
        desc: &ShaderModuleDesc<'_>,
    ) -> Result<ShaderModuleHandle, HalError> {
        // MSL and nothing else. `desc.spirv` and `desc.wgsl` are ignored rather
        // than translated: a SPIR-V→MSL frontend inside this backend would be a
        // second shader compiler solving a problem `crcbl-shaders` already
        // solved offline, with a pinned compiler and a hashed artifact.
        let Some(msl) = desc.msl else {
            return Err(desc.unusable(ShaderSources::MSL));
        };
        let label = desc.label.unwrap_or("<unlabelled>");
        let library = self
            .inner
            .raw
            .newLibraryWithSource_options_error(&NSString::from_str(msl), None)
            .map_err(|error| {
                HalError::ShaderCompilation(format!(
                    "MTLDevice::newLibraryWithSource:options:error: rejected `{label}`: {error}"
                ))
            })?;
        if let Some(label) = desc.label {
            library.setLabel(Some(&NSString::from_str(label)));
        }
        let handle = self.state().shader_modules.insert(ShaderModuleEntry {
            owner: self.inner.id,
            raw: library,
        });
        Ok(self.stamp(handle))
    }

    /// Places every set of a pipeline layout in Metal's argument tables, and the
    /// push-constant block after them.
    ///
    /// See `crcbl_mtl::binding` for the flattening rule and the evidence behind
    /// it, and `crcbl_mtl::argument` for the block. **The order of the two calls
    /// below is the whole push-constant rule**: Slang lowers the block to an
    /// ordinary buffer argument numbered in declaration order, and
    /// `crcbl-shaders` requires every source to declare its push constant last —
    /// so its index is the buffer total the sets leave behind, which is why
    /// `plan` is handed that total rather than a number written down here.
    pub(crate) fn create_pipeline_layout_impl(
        &self,
        desc: &PipelineLayoutDesc<'_>,
    ) -> Result<PipelineLayoutHandle, HalError> {
        let ceiling = self.inner.caps.limits.max_bind_groups as usize;
        if desc.bind_group_layouts.len() > ceiling {
            return Err(HalError::InvalidDescriptor(format!(
                "{} bind group layouts exceed this device's limit of {ceiling}",
                desc.bind_group_layouts.len()
            )));
        }
        let (sets, occupied) = {
            let state = self.state();
            let mut totals = Vec::with_capacity(desc.bind_group_layouts.len());
            for handle in desc.bind_group_layouts {
                let record = lookup(
                    &state.bind_group_layouts,
                    "bind group layout",
                    *handle,
                    &*self.inner,
                )?;
                totals.push((*handle, record.plan.totals));
            }
            crate::binding::plan_layout(&totals)?
        };
        let push_constants = crate::argument::plan(
            desc.push_constants,
            occupied[crate::binding::Table::Buffer.slot()],
            &self.inner.caps,
        )?;
        let handle = self.state().pipeline_layouts.insert(PipelineLayoutEntry {
            owner: self.inner.id,
            sets,
            push_constants,
        });
        Ok(self.stamp(handle))
    }

    /// Builds an `MTLRenderPipelineState`, and keeps the state Metal left on
    /// the encoder.
    pub(crate) fn create_graphics_pipeline_impl(
        &self,
        desc: &GraphicsPipelineDesc<'_>,
    ) -> Result<GraphicsPipelineHandle, HalError> {
        self.check_pipeline_layout(desc.layout)?;
        // A pipeline with no fragment stage is Metal's depth-only pipeline, and
        // it must declare no colour attachment: Metal raises on a nil fragment
        // function beside a colour format. The seam already calls that
        // combination out ("`None` for a depth-only pass"), so this is a caller
        // bug being caught before it becomes an abort.
        if desc.fragment.is_none() && !desc.color_targets.is_empty() {
            return Err(HalError::InvalidDescriptor(format!(
                "a pipeline with no fragment stage declares {} colour target(s); Metal permits a \
                 nil fragment function only for a depth-only pipeline",
                desc.color_targets.len()
            )));
        }
        let ceiling = self.inner.caps.limits.max_color_attachments as usize;
        if desc.color_targets.len() > ceiling {
            return Err(HalError::InvalidDescriptor(format!(
                "{} colour targets exceed this device's limit of {ceiling}",
                desc.color_targets.len()
            )));
        }
        // The other half of `crcbl_mtl::quirk`'s withheld `Features::DEPTH_CLAMP`,
        // and the half without which withholding it would be worse than not: a
        // backend that declares the capability absent and then sets
        // `MTLDepthClipMode::Clamp` anyway has moved the untruth from the
        // adapter to the encoder, where nothing observes it.
        crate::quirk::check_depth_clamp(desc.primitive, self.inner.caps.features)?;
        let samples = check_multisample(&desc.multisample)?;

        let descriptor = MTLRenderPipelineDescriptor::new();
        if let Some(label) = desc.label {
            descriptor.setLabel(Some(&NSString::from_str(label)));
        }
        let vertex = self.function(desc.vertex, "vertex")?;
        descriptor.setVertexFunction(Some(&vertex));
        let fragment = match desc.fragment {
            Some(entry) => Some(self.function(entry, "fragment")?),
            None => None,
        };
        descriptor.setFragmentFunction(fragment.as_deref());
        descriptor.setRasterSampleCount(to_ns(u64::from(samples)));
        descriptor.setAlphaToCoverageEnabled(desc.multisample.alpha_to_coverage);

        for (index, target) in desc.color_targets.iter().enumerate() {
            if target.format.is_depth_stencil() {
                return Err(HalError::InvalidDescriptor(format!(
                    "colour target {index} is {:?}, which is a depth/stencil format",
                    target.format
                )));
            }
            // SAFETY: `objc2` marks the subscript unsafe because Metal does not
            // bounds-check the attachment index. `index` was just bounded by
            // `ceiling`, this device's `max_color_attachments`, which sits at
            // the seam's floor and so is at or below the array's own length —
            // the same argument `begin_render_pass` makes for the render pass
            // descriptor's identical array.
            let slot = unsafe {
                descriptor
                    .colorAttachments()
                    .objectAtIndexedSubscript(index)
            };
            slot.setPixelFormat(conv::pixel_format(target.format));
            slot.setWriteMask(conv::color_write_mask(target.write_mask));
            if let Some(blend) = target.blend {
                slot.setBlendingEnabled(true);
                slot.setSourceRGBBlendFactor(conv::blend_factor(blend.color_src));
                slot.setDestinationRGBBlendFactor(conv::blend_factor(blend.color_dst));
                slot.setRgbBlendOperation(conv::blend_operation(blend.color_op));
                slot.setSourceAlphaBlendFactor(conv::blend_factor(blend.alpha_src));
                slot.setDestinationAlphaBlendFactor(conv::blend_factor(blend.alpha_dst));
                slot.setAlphaBlendOperation(conv::blend_operation(blend.alpha_op));
            }
        }

        let depth_stencil = match desc.depth_stencil {
            // The device's shared always-pass state, not nil. Which object a
            // no-depth pipeline binds is decided here, once per pipeline,
            // rather than at every bind — see
            // [`DeviceInner::default_depth_stencil`].
            None => self.inner.default_depth_stencil.clone(),
            Some(state) => {
                if !state.format.is_depth_stencil() {
                    return Err(HalError::InvalidDescriptor(format!(
                        "DepthStencilState::format is {:?}, which has no depth plane",
                        state.format
                    )));
                }
                descriptor.setDepthAttachmentPixelFormat(conv::pixel_format(state.format));
                // Keyed off the format exactly as `begin_render_pass` is: a
                // `D32Float` pipeline has no stencil plane to declare, and
                // declaring one whose format has no stencil raises.
                if state.format.has_stencil() {
                    descriptor.setStencilAttachmentPixelFormat(conv::pixel_format(state.format));
                } else if state.stencil.is_some() {
                    return Err(HalError::InvalidDescriptor(format!(
                        "a stencil state on a {:?} pipeline, which has no stencil plane to test",
                        state.format
                    )));
                }
                self.depth_stencil_state(&state, desc.label)?
            }
        };

        let label = desc.label.unwrap_or("<unlabelled>");
        let raw = self
            .inner
            .raw
            .newRenderPipelineStateWithDescriptor_error(&descriptor)
            .map_err(|error| {
                HalError::PipelineCreation(format!(
                    "MTLDevice::newRenderPipelineStateWithDescriptor:error: rejected `{label}`: \
                     {error}"
                ))
            })?;

        let primitive = desc.primitive;
        let handle = self
            .state()
            .graphics_pipelines
            .insert(GraphicsPipelineEntry {
                owner: self.inner.id,
                raw,
                depth_stencil,
                raster: RasterState {
                    primitive: conv::primitive_type(primitive.topology),
                    cull: conv::cull_mode(primitive.cull_mode),
                    winding: conv::winding(primitive.front_face),
                    fill: conv::fill_mode(primitive.polygon_mode),
                    clip: conv::depth_clip_mode(primitive.depth_clamp),
                    bias: desc.depth_stencil.map_or([0.0; 3], |state| {
                        [
                            state.bias.constant,
                            state.bias.slope_scale,
                            state.bias.clamp,
                        ]
                    }),
                },
            });
        Ok(self.stamp(handle))
    }

    /// Builds an `MTLComputePipelineState`.
    ///
    /// The descriptor form rather than
    /// `newComputePipelineStateWithFunction:error:`, because only the
    /// descriptor carries a label — which is what names the pipeline in an
    /// Xcode GPU capture, and this backend labels every object it can.
    ///
    /// # The workgroup size is checked here, twice
    ///
    /// This is the backend
    /// [`ComputePipelineDesc::workgroup_size`](crcbl_hal::ComputePipelineDesc::workgroup_size)
    /// exists for, and the one that cannot see what the shader declared: MSL's
    /// `[[kernel]]` says nothing about thread counts. So both checks that *are*
    /// possible happen at creation, where an error can still be returned —
    /// against the seam's limits, and against the compiled kernel's own
    /// `maxTotalThreadsPerThreadgroup`, which Metal derives from its register
    /// use and which a dispatch exceeding raises on. A raise from Objective-C
    /// aborts the process, so catching it here is the difference between an
    /// error a caller handles and a dead one.
    pub(crate) fn create_compute_pipeline_impl(
        &self,
        desc: &ComputePipelineDesc<'_>,
    ) -> Result<ComputePipelineHandle, HalError> {
        self.check_pipeline_layout(desc.layout)?;
        desc.check_workgroup_size(&self.inner.caps.limits)?;
        let descriptor = MTLComputePipelineDescriptor::new();
        let function = self.function(desc.compute, "compute")?;
        descriptor.setComputeFunction(Some(&function));
        if let Some(label) = desc.label {
            descriptor.setLabel(Some(&NSString::from_str(label)));
        }
        let label = desc.label.unwrap_or("<unlabelled>");
        let raw = self
            .inner
            .raw
            .newComputePipelineStateWithDescriptor_options_reflection_error(
                &descriptor,
                MTLPipelineOption::None,
                None,
            )
            .map_err(|error| {
                HalError::PipelineCreation(format!(
                    "MTLDevice::newComputePipelineStateWithDescriptor:options:reflection:error: \
                     rejected `{label}`: {error}"
                ))
            })?;

        let [x, y, z] = desc.workgroup_size;
        let invocations = u64::from(x) * u64::from(y) * u64::from(z);
        let allowed = raw.maxTotalThreadsPerThreadgroup() as u64;
        if invocations > allowed {
            return Err(HalError::PipelineCreation(format!(
                "`{label}` asks for {invocations} threads per threadgroup ({:?}), and this \
                 kernel's maxTotalThreadsPerThreadgroup is {allowed} — Metal raises at the \
                 dispatch rather than reporting it, and a raise aborts the process",
                desc.workgroup_size
            )));
        }

        let handle = self.state().compute_pipelines.insert(ComputePipelineEntry {
            owner: self.inner.id,
            raw,
            threads_per_threadgroup: MTLSize {
                width: to_ns(u64::from(x)),
                height: to_ns(u64::from(y)),
                depth: to_ns(u64::from(z)),
            },
        });
        Ok(self.stamp(handle))
    }

    /// Resolves a [`ShaderEntry`] to the named `MTLFunction`.
    ///
    /// Metal looks a function up by name and answers nil for a miss, with no
    /// diagnostic of its own — so the message here is the only thing that says
    /// *which* name was not found, and in which stage's slot it was wanted.
    fn function(
        &self,
        entry: ShaderEntry<'_>,
        stage: &str,
    ) -> Result<Retained<ProtocolObject<dyn MTLFunction>>, HalError> {
        let library = {
            let state = self.state();
            lookup(
                &state.shader_modules,
                "shader module",
                entry.module,
                &*self.inner,
            )?
            .raw
            .clone()
        };
        library
            .newFunctionWithName(&NSString::from_str(entry.entry_point))
            .ok_or_else(|| {
                HalError::ShaderCompilation(format!(
                    "the module has no function named `{}` for the {stage} stage; Metal resolves \
                     an entry point by name, and the MSL artifact's names are the ones \
                     spirv/manifest.txt recorded from OpEntryPoint",
                    entry.entry_point
                ))
            })
    }

    /// Turns the seam's depth/stencil state into the `MTLDepthStencilState` the
    /// encoder binds.
    fn depth_stencil_state(
        &self,
        state: &DepthStencilState,
        label: Option<&str>,
    ) -> Result<Retained<ProtocolObject<dyn MTLDepthStencilState>>, HalError> {
        let descriptor = MTLDepthStencilDescriptor::new();
        // `compare_function` performs no reversed-Z flip; see `conv`.
        descriptor.setDepthCompareFunction(conv::compare_function(state.depth_compare));
        descriptor.setDepthWriteEnabled(state.depth_write);
        if let Some(stencil) = state.stencil {
            descriptor.setFrontFaceStencil(Some(&stencil_face(&stencil, true)));
            descriptor.setBackFaceStencil(Some(&stencil_face(&stencil, false)));
        }
        if let Some(label) = label {
            descriptor.setLabel(Some(&NSString::from_str(label)));
        }
        self.inner
            .raw
            .newDepthStencilStateWithDescriptor(&descriptor)
            .ok_or_else(|| {
                HalError::PipelineCreation(
                    "MTLDevice::newDepthStencilStateWithDescriptor: returned nil".to_string(),
                )
            })
    }

    /// Checks that a pipeline layout handle is one this device issued.
    ///
    /// The layout carries nothing a pipeline needs while it can only be empty,
    /// so this is the whole of what a pipeline does with it — and it is not
    /// ceremony: obligation 3 says a handle from another device must be
    /// [`HalError::ForeignObject`] rather than silently accepted.
    fn check_pipeline_layout(&self, layout: PipelineLayoutHandle) -> Result<(), HalError> {
        let state = self.state();
        lookup(
            &state.pipeline_layouts,
            "pipeline layout",
            layout,
            &*self.inner,
        )?;
        Ok(())
    }

    /// Removes a pipeline-family object from its pool.
    pub(crate) fn destroy_shader_module_impl(&self, module: ShaderModuleHandle) {
        let mut state = self.state();
        crate::device::take_owned(&mut state.shader_modules, module, &*self.inner);
    }

    pub(crate) fn destroy_pipeline_layout_impl(&self, layout: PipelineLayoutHandle) {
        let mut state = self.state();
        crate::device::take_owned(&mut state.pipeline_layouts, layout, &*self.inner);
    }

    pub(crate) fn destroy_graphics_pipeline_impl(&self, pipeline: GraphicsPipelineHandle) {
        let mut state = self.state();
        crate::device::take_owned(&mut state.graphics_pipelines, pipeline, &*self.inner);
    }

    pub(crate) fn destroy_compute_pipeline_impl(&self, pipeline: ComputePipelineHandle) {
        let mut state = self.state();
        crate::device::take_owned(&mut state.compute_pipelines, pipeline, &*self.inner);
    }
}

/// The `MTLDepthStencilState` a pipeline that declares none is bound with, so
/// that `setDepthStencilState:` is never handed nil.
///
/// # Why nil is not an option
///
/// `setDepthStencilState:nil` **hangs** GitHub's `Apple Paravirtual device`:
/// every draw this backend recorded faulted there with
/// `kIOGPUCommandBufferCallbackErrorHang` while a render-pass clear read back
/// correctly, and a ten-probe bisect on `6a59e89` narrowed it to that one call
/// with that one argument. A hand-encoded pass plus `setDepthStencilState:nil`
/// hung; the same pass plus a real state object passed; each of the other five
/// rasteriser calls passed on its own. Metal documents nil as "reset to the
/// default", so this is a driver bug rather than misuse — but a bug on a device
/// the project ships to, and the substitution costs one object per device.
///
/// # Why it is a no-op for rendering
///
/// What nil is documented to restore is "the depth test always passes and depth
/// writes are disabled", and this builds exactly that — but it **sets** every
/// field rather than trusting a default. `objc2-metal` is a generated ABI
/// binding: `MTLDepthStencilDescriptor::new` is `[MTLDepthStencilDescriptor
/// new]` and nothing in the crate states what the fields come back as, so
/// leaving one unset would be relying on a value that is not written down
/// anywhere this workspace can read.
///
/// * `depthCompareFunction` is `Always` and `depthWriteEnabled` is false, which
///   is the documented no-state behaviour verbatim.
/// * Both facings compare `Always` and keep on all three outcomes, so the
///   stencil test rejects nothing and no stencil write happens. The read and
///   write masks are left alone deliberately: `Always` reads no bits and `Keep`
///   writes none, so neither mask can be observed whatever it defaults to.
///
/// Which makes the state unobservable in the image for every pipeline it is
/// bound for — and it is bound for exactly the pipelines whose
/// [`GraphicsPipelineDesc::depth_stencil`] is `None`, which declare no depth or
/// stencil attachment format at all.
///
/// # Errors
///
/// [`HalError::Backend`] when `newDepthStencilStateWithDescriptor:` returns
/// nil, which fails device creation rather than leaving a device that cannot
/// bind a pipeline.
pub(crate) fn default_depth_stencil_state(
    raw: &ProtocolObject<dyn MTLDevice>,
) -> Result<Retained<ProtocolObject<dyn MTLDepthStencilState>>, HalError> {
    let face = MTLStencilDescriptor::new();
    face.setStencilCompareFunction(MTLCompareFunction::Always);
    face.setStencilFailureOperation(MTLStencilOperation::Keep);
    face.setDepthFailureOperation(MTLStencilOperation::Keep);
    face.setDepthStencilPassOperation(MTLStencilOperation::Keep);

    let descriptor = MTLDepthStencilDescriptor::new();
    descriptor.setDepthCompareFunction(MTLCompareFunction::Always);
    descriptor.setDepthWriteEnabled(false);
    // Both setters copy, so one descriptor can serve both facings.
    descriptor.setFrontFaceStencil(Some(&face));
    descriptor.setBackFaceStencil(Some(&face));
    descriptor.setLabel(Some(&NSString::from_str(
        "crcbl-mtl default depth/stencil (always, no write)",
    )));

    raw.newDepthStencilStateWithDescriptor(&descriptor)
        .ok_or_else(|| {
            HalError::Backend(
                "MTLDevice::newDepthStencilStateWithDescriptor: returned nil for the default \
                 always-pass state, which every pipeline without a depth/stencil state binds"
                    .to_string(),
            )
        })
}

/// One facing's stencil state.
///
/// The seam carries the read and write masks once for both facings while Metal
/// carries them per facing, so both copies get the same pair — which is what
/// the seam's single field means.
fn stencil_face(stencil: &StencilState, front: bool) -> Retained<MTLStencilDescriptor> {
    let face = if front { stencil.front } else { stencil.back };
    let descriptor = MTLStencilDescriptor::new();
    descriptor.setStencilCompareFunction(conv::compare_function(face.compare));
    descriptor.setStencilFailureOperation(conv::stencil_operation(face.fail_op));
    descriptor.setDepthFailureOperation(conv::stencil_operation(face.depth_fail_op));
    descriptor.setDepthStencilPassOperation(conv::stencil_operation(face.pass_op));
    descriptor.setReadMask(stencil.read_mask);
    descriptor.setWriteMask(stencil.write_mask);
    descriptor
}

/// The sample count a pipeline rasterises at, or why the state has no Metal
/// encoding.
///
/// **Metal has no per-pipeline sample mask.**
/// [`MultisampleState::mask`](crcbl_hal::MultisampleState::mask) is Vulkan's
/// `pSampleMask` and DX12's `SampleMask`; `MTLRenderPipelineDescriptor` has no
/// counterpart at all, and the only related knob — `alphaToCoverageEnabled` —
/// derives coverage from the shader instead of masking it. So a mask that is
/// not "every sample" is refused by name rather than silently ignored, which
/// would render every sample a caller asked to suppress.
fn check_multisample(multisample: &MultisampleState) -> Result<u32, HalError> {
    let samples = multisample.samples.max(1);
    // Only the bits a sample actually uses have to be set: a 4× pipeline with
    // mask `0xF` is asking for every one of its samples, however the caller
    // spelled the unused high bits.
    let used = if samples >= u32::BITS {
        u32::MAX
    } else {
        (1u32 << samples) - 1
    };
    if multisample.mask & used != used {
        return Err(HalError::InvalidDescriptor(format!(
            "MultisampleState::mask {:#010x} suppresses samples of a {samples}× pipeline, and \
             Metal has no sample mask on MTLRenderPipelineDescriptor to express it",
            multisample.mask
        )));
    }
    Ok(samples)
}
