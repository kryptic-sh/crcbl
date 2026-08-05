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
//! # A pipeline layout can only be empty in this slice, and says so
//!
//! [`create_bind_group_layout`](crcbl_hal::Device::create_bind_group_layout)
//! still refuses — the binding slice owns the argument-buffer decision — so no
//! [`BindGroupLayoutHandle`](crcbl_hal::BindGroupLayoutHandle) exists for a
//! layout to name, and
//! [`create_pipeline_layout`](crcbl_hal::Device::create_pipeline_layout)
//! accepts exactly the empty one. That is not a token gesture: a pipeline whose
//! shaders read no resources needs no bindings at all, and it is what makes the
//! triangle in `device.rs`'s test a real draw through the real seam.
//!
//! A push-constant range is refused for a different reason and with a different
//! error: [`Features::PUSH_CONSTANTS`](crcbl_hal::Features::PUSH_CONSTANTS) is
//! absent from this backend's caps, and `crcbl_hal::pipeline` requires a
//! backend without it to fail **at layout creation** rather than dropping every
//! `push_constants` call silently.
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
    MTLComputePipelineDescriptor, MTLComputePipelineState, MTLCullMode, MTLDepthClipMode,
    MTLDepthStencilDescriptor, MTLDepthStencilState, MTLDevice, MTLFunction, MTLLibrary,
    MTLPipelineOption, MTLPrimitiveType, MTLRenderPipelineDescriptor, MTLRenderPipelineState,
    MTLStencilDescriptor, MTLTriangleFillMode, MTLWinding,
};

use crate::conv;
use crate::device::{DeviceInner, MetalDevice, Owned, lookup, owned, to_ns};

/// A compiled `MTLLibrary`, and the functions a pipeline pulls out of it.
#[derive(Debug)]
pub(crate) struct ShaderModuleEntry {
    pub(crate) owner: u64,
    pub(crate) raw: Retained<ProtocolObject<dyn MTLLibrary>>,
}

/// A pipeline layout.
///
/// It holds nothing but its owner, and that is the whole of what an empty
/// layout is on Metal: there are no descriptor set layouts to combine and no
/// root signature to build. It exists as a pool entry rather than a synthesised
/// handle so that a pipeline naming another device's layout is caught as
/// [`HalError::ForeignObject`], which obligation 3 requires and which a handle
/// nobody allocated could not provide.
#[derive(Debug)]
pub(crate) struct PipelineLayoutEntry {
    pub(crate) owner: u64,
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
    /// `setStencilReferenceValue:`, or `None` when the pipeline declares no
    /// stencil state. `Some(0)` and `None` are different: the first overwrites
    /// whatever an earlier `set_stencil_reference` left, the second leaves it.
    pub(crate) stencil_reference: Option<u32>,
}

/// A graphics pipeline: the Metal object, plus the state Metal would not take.
#[derive(Debug)]
pub(crate) struct GraphicsPipelineEntry {
    pub(crate) owner: u64,
    pub(crate) raw: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    /// `None` for a pipeline with no depth/stencil state, which binds nil and
    /// so restores Metal's default: always pass, never write.
    pub(crate) depth_stencil: Option<Retained<ProtocolObject<dyn MTLDepthStencilState>>>,
    pub(crate) raster: RasterState,
}

/// A compute pipeline.
#[derive(Debug)]
pub(crate) struct ComputePipelineEntry {
    pub(crate) owner: u64,
    /// Held so the handle keeps the pipeline alive. Nothing reads it back until
    /// `bind_compute_pipeline` lands, which needs an
    /// `MTLComputeCommandEncoder` this slice does not open.
    #[allow(dead_code)]
    pub(crate) raw: Retained<ProtocolObject<dyn MTLComputePipelineState>>,
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
    pub(crate) depth_stencil: Option<Retained<ProtocolObject<dyn MTLDepthStencilState>>>,
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

    /// Creates the empty pipeline layout, and refuses everything else by name.
    pub(crate) fn create_pipeline_layout_impl(
        &self,
        desc: &PipelineLayoutDesc<'_>,
    ) -> Result<PipelineLayoutHandle, HalError> {
        if let Some(range) = desc.push_constants {
            // `crcbl_hal::pipeline` requires this to fail here rather than at
            // the `push_constants` call, so a caller learns at start-up instead
            // of watching every draw read stale constants.
            return Err(HalError::InvalidDescriptor(format!(
                "a push-constant range of {} bytes needs Features::PUSH_CONSTANTS, which this \
                 device does not report: `setVertexBytes:length:atIndex:` is Metal's closest fit \
                 and which buffer index it would occupy is a binding-slice decision",
                range.size
            )));
        }
        if !desc.bind_group_layouts.is_empty() {
            return Err(crate::MetalInstance::not_yet(
                "pipeline layouts with bind groups (the Metal binding slice)",
            ));
        }
        let handle = self.state().pipeline_layouts.insert(PipelineLayoutEntry {
            owner: self.inner.id,
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
            None => None,
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
                Some(self.depth_stencil_state(&state, desc.label)?)
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
                    stencil_reference: desc
                        .depth_stencil
                        .and_then(|state| state.stencil)
                        .map(|stencil| stencil.reference),
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
    pub(crate) fn create_compute_pipeline_impl(
        &self,
        desc: &ComputePipelineDesc<'_>,
    ) -> Result<ComputePipelineHandle, HalError> {
        self.check_pipeline_layout(desc.layout)?;
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
        let handle = self.state().compute_pipelines.insert(ComputePipelineEntry {
            owner: self.inner.id,
            raw,
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
                &self.inner,
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
            &self.inner,
        )?;
        Ok(())
    }

    /// Removes a pipeline-family object from its pool.
    pub(crate) fn destroy_shader_module_impl(&self, module: ShaderModuleHandle) {
        let mut state = self.state();
        crate::device::take_owned(&mut state.shader_modules, module, &self.inner);
    }

    pub(crate) fn destroy_pipeline_layout_impl(&self, layout: PipelineLayoutHandle) {
        let mut state = self.state();
        crate::device::take_owned(&mut state.pipeline_layouts, layout, &self.inner);
    }

    pub(crate) fn destroy_graphics_pipeline_impl(&self, pipeline: GraphicsPipelineHandle) {
        let mut state = self.state();
        crate::device::take_owned(&mut state.graphics_pipelines, pipeline, &self.inner);
    }

    pub(crate) fn destroy_compute_pipeline_impl(&self, pipeline: ComputePipelineHandle) {
        let mut state = self.state();
        crate::device::take_owned(&mut state.compute_pipelines, pipeline, &self.inner);
    }
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
