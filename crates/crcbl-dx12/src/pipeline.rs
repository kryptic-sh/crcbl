//! Shader modules, root signatures and pipeline state objects — the rung that
//! turns a clear into a triangle.
//!
//! # A shader module is a set of *signed DXIL containers*, one per entry point
//!
//! `crcbl-shaders` commits `dxil/<shader>.<entry>.dxil`, and
//! [`create_shader_module`](crcbl_hal::Device::create_shader_module) takes the
//! bytes and hands them to `D3D12_SHADER_BYTECODE`. There is no compile step
//! here at all — the opposite end of the range from `crcbl-mtl`, which compiles
//! MSL *source* at device-init time because Apple's offline compiler ships only
//! with Xcode.
//!
//! **`dxc` compiles a single `-E` per invocation, so a container is one entry
//! point.** That is why `crcbl_shaders::Shader::dxil` takes an entry-point name.
//! It is not why a caller would create two modules, though: the seam's
//! [`dxil`](crcbl_hal::ShaderModuleDesc::dxil) field carries a container *per
//! entry point*, so a caller drawing with a vertex and a fragment stage creates
//! one module here exactly as it does on the other three backends, and
//! [`ShaderModuleEntry::container`] picks the container the stage names. That
//! keeps the format's granularity inside this backend, which is the only place
//! it means anything.
//!
//! # The container is validated in [`crate::dxil`], because the alternative is a
//! driver rejecting it
//!
//! That module parses the fixed part of the `DxilContainerHeader` — the format's
//! own layout, not this crate's — and refuses:
//!
//! * bytes that do not open with the `DXBC` magic, or whose declared container
//!   size is not the blob's, which is a truncated or mis-sliced artifact;
//! * an **all-zero digest**, which means `dxc` never loaded `libdxil.so` and the
//!   container is unsigned. Every real D3D12 driver refuses an unsigned
//!   container, and it would refuse it at `CreateGraphicsPipelineState` with a
//!   message about the pipeline rather than about the artifact;
//! * a container whose `DXIL` part declares the **wrong shader kind** for the
//!   stage slot it is being put in. `CreateGraphicsPipelineState` would also
//!   catch a pixel container in the vertex slot, but not as
//!   "`vertexMain`'s container is a pixel shader".
//!
//! `crcbl-shaders`' own `every_shipped_shader_has_a_signed_dxil_container_per_entry_point`
//! asserts the same three properties over every committed artifact with no
//! Windows in the loop. This is the check on the *caller's* bytes, which need
//! not have come from there.
//!
//! It lives there rather than here because it holds no `windows` type, so it is
//! the half of this file a Linux box can run — which is also where the
//! thread-group size a compute pipeline is checked against is read from.
//!
//! # A pipeline layout is a root signature, and the sets are its parameters
//!
//! Each bind group layout becomes one or two root parameters — a CBV/SRV/UAV
//! descriptor table and a sampler table, which D3D12 will not let share one —
//! and `crcbl_dx12::binding` computes the ranges. [`SetPlacement`] records which
//! root parameter index each half landed at, because
//! [`bind_group`](crcbl_hal::CommandEncoder::bind_group) is given a set index
//! and `SetGraphicsRootDescriptorTable` takes a root parameter index, and the
//! two are only the same number when every set has exactly one table.
//!
//! Root signature **1.0** is serialised, deliberately: its descriptor ranges are
//! volatile by definition, which is what
//! [`BindingFlags::UPDATE_AFTER_BIND`](crcbl_hal::BindingFlags::UPDATE_AFTER_BIND)
//! asks for, and 1.1 would add a `CheckFeatureSupport` call and a fallback path
//! to buy static-descriptor optimisations nothing here measures.
//!
//! A push-constant range is refused, and at *layout creation*:
//! [`Features::PUSH_CONSTANTS`](crcbl_hal::Features::PUSH_CONSTANTS) is not in
//! this backend's caps — `crcbl_dx12::adapter` reports it nowhere — and
//! `crcbl_hal::pipeline` requires a backend without it to fail there rather than
//! dropping every `push_constants` call silently. D3D12 *has* the feature under
//! the name root constants; what it does not have is a way for this backend to
//! know which root parameter slot the committed artifacts put one at, which is
//! the same gap `crcbl-mtl` names for `setVertexBytes:`.
//!
//! # D3D12 keeps almost all of the pipeline in the object, and one thing outside
//!
//! [`GraphicsPipelineDesc`](crcbl_hal::GraphicsPipelineDesc) is Vulkan-shaped
//! and `D3D12_GRAPHICS_PIPELINE_STATE_DESC` is very nearly the same object:
//! blend, depth/stencil, rasteriser, attachment formats and sample state are all
//! in it, unlike Metal, which leaves half on the encoder. The exception is the
//! **primitive topology**: the PSO takes a *category* (point, line, triangle)
//! and the command list takes the exact topology, so [`GraphicsPipelineEntry`]
//! carries the second value and
//! [`bind_graphics_pipeline`](crcbl_hal::CommandEncoder::bind_graphics_pipeline)
//! replays it. The stencil reference is the other, and travels the same way.
//!
//! **There is no input layout, and that is the architecture.** The seam has no
//! vertex-buffer layout and no `bind_vertex_buffer` — geometry is pulled from
//! storage buffers — so `InputLayout` is empty and the root signature does not
//! set `ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT`. See `crcbl_hal::pipeline`.
//!
//! # Reversed-Z crosses unchanged
//!
//! [`CompareOp::Greater`](crcbl_hal::CompareOp) reaches
//! `D3D12_COMPARISON_FUNC_GREATER` through `conv::comparison_func`, and the
//! viewport's depth range is left alone. Both are the engine's locked convention
//! (`crcbl_hal::depth`), produced by the projection matrix above this seam; a
//! backend that "corrected" either would apply it twice.

use core::mem::ManuallyDrop;

use crcbl_hal::{
    ColorTargetState, ComputePipelineDesc, DepthStencilState, GraphicsPipelineDesc, HalError,
    MultisampleState, PipelineLayoutDesc, PrimitiveState, ShaderStages,
};
use windows::Win32::Graphics::Direct3D::{D3D_PRIMITIVE_TOPOLOGY, ID3DBlob};
use windows::Win32::Graphics::Direct3D12::{
    D3D_ROOT_SIGNATURE_VERSION_1, D3D12_BLEND_DESC, D3D12_COMPARISON_FUNC_ALWAYS,
    D3D12_COMPUTE_PIPELINE_STATE_DESC, D3D12_CONSERVATIVE_RASTERIZATION_MODE_OFF,
    D3D12_DEPTH_STENCIL_DESC, D3D12_DEPTH_STENCILOP_DESC, D3D12_DEPTH_WRITE_MASK_ALL,
    D3D12_DEPTH_WRITE_MASK_ZERO, D3D12_DESCRIPTOR_RANGE, D3D12_GRAPHICS_PIPELINE_STATE_DESC,
    D3D12_INDEX_BUFFER_STRIP_CUT_VALUE_DISABLED, D3D12_INPUT_LAYOUT_DESC, D3D12_LOGIC_OP_NOOP,
    D3D12_PIPELINE_STATE_FLAG_NONE, D3D12_RASTERIZER_DESC, D3D12_RENDER_TARGET_BLEND_DESC,
    D3D12_ROOT_DESCRIPTOR_TABLE, D3D12_ROOT_PARAMETER, D3D12_ROOT_PARAMETER_0,
    D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE, D3D12_ROOT_SIGNATURE_DESC,
    D3D12_ROOT_SIGNATURE_FLAG_NONE, D3D12SerializeRootSignature, ID3D12Device, ID3D12PipelineState,
    ID3D12RootSignature,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC};

use crate::conv;
use crate::dxil::{Dxil, ShaderModuleEntry};
use crate::handle::Owned;

/// Colour attachments a D3D12 pipeline state object can declare.
///
/// The array's own length in `D3D12_GRAPHICS_PIPELINE_STATE_DESC`, which is what
/// makes a longer `color_targets` a refusal rather than a silent truncation.
const RENDER_TARGETS: usize = 8;

/// Where one bind group set's tables landed among the root parameters.
///
/// A set is one root parameter when it declares only views or only samplers, and
/// two when it declares both — so the set index and the root parameter index are
/// not the same number, and this is what keeps them apart.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SetPlacement {
    /// Root parameter index of the CBV/SRV/UAV table, if the set has one.
    pub(crate) views: Option<u32>,
    /// Root parameter index of the sampler table, if the set has one.
    pub(crate) samplers: Option<u32>,
}

/// A pipeline layout: the root signature, and where each set's tables are.
#[derive(Debug)]
pub(crate) struct PipelineLayoutEntry {
    pub(crate) owner: u64,
    pub(crate) raw: ID3D12RootSignature,
    /// In set order, so `sets[n]` is what `bind_group(n, …)` binds into.
    pub(crate) sets: Vec<SetPlacement>,
    /// The bind group layouts this signature was built from, in set order.
    ///
    /// Kept so `bind_group` can check the group it is given conforms to the
    /// layout the root signature declares — a group of the wrong shape binds a
    /// table of the wrong length, which D3D12 reads as arithmetic and never
    /// reports.
    pub(crate) layouts: Vec<crcbl_hal::BindGroupLayoutHandle>,
}

/// A graphics pipeline: the state object, plus what D3D12 left on the encoder.
#[derive(Debug)]
pub(crate) struct GraphicsPipelineEntry {
    pub(crate) owner: u64,
    pub(crate) raw: ID3D12PipelineState,
    /// The root signature the pipeline was built against.
    ///
    /// `SetPipelineState` does not set it — a command list carries a root
    /// signature of its own — so binding a pipeline must set both, and this is
    /// how the encoder reaches the right one without resolving the layout handle
    /// it was never given.
    pub(crate) root_signature: ID3D12RootSignature,
    /// `IASetPrimitiveTopology`, which the PSO takes only the category of.
    pub(crate) topology: D3D_PRIMITIVE_TOPOLOGY,
    /// `OMSetStencilRef`, or `None` for a pipeline that declares no stencil
    /// state. `Some(0)` and `None` differ: the first overwrites whatever an
    /// earlier `set_stencil_reference` left, the second leaves it.
    pub(crate) stencil_reference: Option<u32>,
}

/// A compute pipeline: the state object and the signature it was built against.
///
/// No topology and no stencil reference, which is the whole difference from
/// [`GraphicsPipelineEntry`]: D3D12 leaves nothing about a dispatch on the
/// command list except the root arguments every pipeline needs.
#[derive(Debug)]
pub(crate) struct ComputePipelineEntry {
    pub(crate) owner: u64,
    pub(crate) raw: ID3D12PipelineState,
    /// As [`GraphicsPipelineEntry::root_signature`], and set through
    /// `SetComputeRootSignature` rather than the graphics call — a command list
    /// carries one of each, and setting the wrong one leaves the dispatch with
    /// whatever the last draw bound.
    pub(crate) root_signature: ID3D12RootSignature,
}

impl Owned for ShaderModuleEntry {
    fn owner(&self) -> u64 {
        self.owner
    }
}

impl Owned for ComputePipelineEntry {
    fn owner(&self) -> u64 {
        self.owner
    }
}

impl Owned for PipelineLayoutEntry {
    fn owner(&self) -> u64 {
        self.owner
    }
}

impl Owned for GraphicsPipelineEntry {
    fn owner(&self) -> u64 {
        self.owner
    }
}

/// One set's root parameters, and the ranges they point at.
///
/// The ranges are returned alongside because `D3D12_ROOT_DESCRIPTOR_TABLE` holds
/// a *pointer* into them: they must outlive the serialisation call, and keeping
/// them in one owned `Vec` per set is what guarantees that without a lifetime
/// nobody can express in an FFI struct.
struct RootPlan {
    parameters: Vec<D3D12_ROOT_PARAMETER>,
    /// One entry per set, in set order.
    sets: Vec<SetPlacement>,
    /// Kept alive for the duration of the serialise call; never read again.
    _ranges: Vec<Vec<D3D12_DESCRIPTOR_RANGE>>,
}

/// Builds a root signature from a pipeline layout's sets.
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`] for a push-constant range (see the module
/// docs) or more sets than
/// [`Limits::max_bind_groups`](crcbl_hal::Limits::max_bind_groups), and
/// [`HalError::Backend`] when D3D12 refuses to serialise or create the
/// signature — carrying the serialiser's own error blob, which is the only text
/// that says *which* parameter it objected to.
pub(crate) fn layout(
    device: &ID3D12Device,
    desc: &PipelineLayoutDesc<'_>,
    sets: &[(crcbl_hal::BindGroupLayoutHandle, RootTables)],
    owner: u64,
) -> Result<PipelineLayoutEntry, HalError> {
    if let Some(range) = desc.push_constants {
        return Err(HalError::InvalidDescriptor(format!(
            "a push-constant range of {} bytes needs Features::PUSH_CONSTANTS, which this device \
             does not report: D3D12's root constants are the equivalent, and nothing here knows \
             which root parameter slot the committed DXIL puts one at (the DX12 root-constant \
             slice)",
            range.size
        )));
    }
    let plan = plan_root(sets);
    let signature = D3D12_ROOT_SIGNATURE_DESC {
        NumParameters: u32::try_from(plan.parameters.len()).unwrap_or(u32::MAX),
        pParameters: plan.parameters.as_ptr(),
        NumStaticSamplers: 0,
        pStaticSamplers: core::ptr::null(),
        // Not `ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT`: the seam has no vertex input
        // state, so no pipeline built from this signature has an input layout to
        // allow. See the module docs.
        Flags: D3D12_ROOT_SIGNATURE_FLAG_NONE,
    };

    let mut blob: Option<ID3DBlob> = None;
    let mut errors: Option<ID3DBlob> = None;
    // SAFETY: `signature` is a live local borrowed for the call. Its
    // `pParameters` points into `plan.parameters`, and each parameter's
    // `pDescriptorRanges` into `plan._ranges`; both outlive this statement,
    // which is the whole reason `RootPlan` owns them together. Both out
    // pointers name live locals.
    let result = unsafe {
        D3D12SerializeRootSignature(
            &raw const signature,
            D3D_ROOT_SIGNATURE_VERSION_1,
            &raw mut blob,
            Some(&raw mut errors),
        )
    };
    if let Err(error) = result {
        return Err(HalError::Backend(format!(
            "D3D12SerializeRootSignature failed for `{}`: {error}{}",
            desc.label.unwrap_or("<unlabelled>"),
            blob_message(errors.as_ref()),
        )));
    }
    let blob = blob.ok_or_else(|| {
        HalError::Backend("D3D12SerializeRootSignature reported success and no blob".to_string())
    })?;
    // SAFETY: `blob` is the blob the call above just wrote, so the pointer and
    // length describe its own live allocation, and the slice is borrowed only
    // for the `CreateRootSignature` call on the next line.
    let serialised = unsafe {
        core::slice::from_raw_parts(blob.GetBufferPointer().cast::<u8>(), blob.GetBufferSize())
    };
    // SAFETY: `device` is live, `serialised` is the blob just serialised, and
    // `ID3D12RootSignature` is the IID asked for. Node zero, as everywhere in
    // this crate.
    let raw: ID3D12RootSignature =
        unsafe { device.CreateRootSignature(0, serialised) }.map_err(|error| {
            HalError::Backend(format!(
                "ID3D12Device::CreateRootSignature failed for `{}`: {error}",
                desc.label.unwrap_or("<unlabelled>")
            ))
        })?;

    Ok(PipelineLayoutEntry {
        owner,
        raw,
        sets: plan.sets,
        layouts: sets.iter().map(|(handle, _)| *handle).collect(),
    })
}

/// The ranges one set contributes, already computed by `crcbl_dx12::binding`.
pub(crate) struct RootTables {
    /// CBV/SRV/UAV ranges, empty when the set declares none.
    pub(crate) views: Vec<D3D12_DESCRIPTOR_RANGE>,
    /// Sampler ranges, empty when the set declares none.
    pub(crate) samplers: Vec<D3D12_DESCRIPTOR_RANGE>,
    /// The union of the set's entry visibilities.
    pub(crate) visibility: ShaderStages,
}

/// Lays every set's tables out as root parameters, in set order.
fn plan_root(sets: &[(crcbl_hal::BindGroupLayoutHandle, RootTables)]) -> RootPlan {
    let mut parameters = Vec::new();
    let mut placements = Vec::with_capacity(sets.len());
    let mut owned: Vec<Vec<D3D12_DESCRIPTOR_RANGE>> = Vec::new();

    for (_, tables) in sets {
        let mut placement = SetPlacement::default();
        for (ranges, slot) in [
            (&tables.views, &mut placement.views),
            (&tables.samplers, &mut placement.samplers),
        ] {
            if ranges.is_empty() {
                continue;
            }
            // Cloned into `owned` and pointed at from there. Pushing another
            // entry later moves the outer `Vec`'s elements but never the inner
            // allocation this pointer names, which is what makes taking the
            // address here sound while the list is still being built.
            owned.push(ranges.clone());
            let held = owned.last().expect("just pushed");
            *slot = Some(u32::try_from(parameters.len()).unwrap_or(u32::MAX));
            parameters.push(D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                        NumDescriptorRanges: u32::try_from(held.len()).unwrap_or(u32::MAX),
                        pDescriptorRanges: held.as_ptr(),
                    },
                },
                ShaderVisibility: conv::shader_visibility(tables.visibility),
            });
        }
        placements.push(placement);
    }

    RootPlan {
        parameters,
        sets: placements,
        _ranges: owned,
    }
}

/// The serialiser's own diagnostic, or nothing.
fn blob_message(errors: Option<&ID3DBlob>) -> String {
    let Some(blob) = errors else {
        return String::new();
    };
    // SAFETY: `blob` is a live `ID3DBlob` the serialiser wrote, so the pointer
    // and length describe its own allocation, and the slice is read before this
    // function returns.
    let bytes = unsafe {
        core::slice::from_raw_parts(blob.GetBufferPointer().cast::<u8>(), blob.GetBufferSize())
    };
    format!(
        " ({})",
        String::from_utf8_lossy(bytes).trim_end_matches('\0')
    )
}

/// Builds a `D3D12_GRAPHICS_PIPELINE_STATE_DESC` and the object from it.
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`] for a descriptor D3D12 cannot express — more
/// colour targets than a pipeline state has slots, a depth format with no depth
/// plane, a stencil state on a format with no stencil plane — and
/// [`HalError::PipelineCreation`] carrying D3D12's own message when the driver
/// refuses the object.
pub(crate) fn graphics(
    device: &ID3D12Device,
    desc: &GraphicsPipelineDesc<'_>,
    layout: &PipelineLayoutEntry,
    vertex: &ShaderModuleEntry,
    fragment: Option<&ShaderModuleEntry>,
    owner: u64,
) -> Result<GraphicsPipelineEntry, HalError> {
    // The container each stage names, out of whatever the module was given.
    // Both stages routinely come out of *one* module — Slang emits a vertex and
    // a fragment entry point into one file, and the seam carries a container per
    // entry point so the caller need not split the module in two.
    let vertex_dxil = vertex.container(desc.vertex.entry_point)?;
    vertex_dxil.expect(ShaderStages::VERTEX, desc.vertex.entry_point)?;
    let fragment_dxil = match (fragment, desc.fragment) {
        (Some(module), Some(entry)) => {
            let dxil = module.container(entry.entry_point)?;
            dxil.expect(ShaderStages::FRAGMENT, entry.entry_point)?;
            Some(dxil)
        }
        _ => None,
    };
    if desc.color_targets.len() > RENDER_TARGETS {
        return Err(HalError::InvalidDescriptor(format!(
            "{} colour targets exceed the {RENDER_TARGETS} a D3D12 pipeline state declares",
            desc.color_targets.len()
        )));
    }
    let samples = desc.multisample.samples.max(1);
    // **Every fallible step happens before the descriptor exists.** The first
    // field written clones a reference into a `ManuallyDrop`, which only
    // `release_root_signature` gives back — so a `?` *inside* the literal would
    // leak that reference on the error path, and the depth/stencil state is the
    // one part of this that can refuse.
    let depth_stencil = depth_stencil_state(desc.depth_stencil.as_ref())?;
    for (index, target) in desc.color_targets.iter().enumerate() {
        if target.format.is_depth_stencil() {
            return Err(HalError::InvalidDescriptor(format!(
                "colour target {index} is {:?}, which is a depth/stencil format",
                target.format
            )));
        }
    }

    let mut state = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
        // `ManuallyDrop` because the field is one: the descriptor borrows the
        // signature for the call and must not release it. `layout` outlives this
        // function, so the reference it clones here is released by the
        // `ManuallyDrop::drop` at the end.
        pRootSignature: ManuallyDrop::new(Some(layout.raw.clone())),
        VS: vertex_dxil.bytecode(),
        PS: fragment_dxil.map(Dxil::bytecode).unwrap_or_default(),
        BlendState: blend_state(desc.color_targets, &desc.multisample),
        SampleMask: desc.multisample.mask,
        RasterizerState: rasterizer_state(&desc.primitive, desc.depth_stencil.as_ref(), samples),
        DepthStencilState: depth_stencil,
        // Empty, and deliberately: vertex pulling is the only geometry path, so
        // there is no vertex input state to declare. See the module docs.
        InputLayout: D3D12_INPUT_LAYOUT_DESC {
            pInputElementDescs: core::ptr::null(),
            NumElements: 0,
        },
        IBStripCutValue: D3D12_INDEX_BUFFER_STRIP_CUT_VALUE_DISABLED,
        PrimitiveTopologyType: conv::primitive_topology_type(desc.primitive.topology),
        NumRenderTargets: u32::try_from(desc.color_targets.len()).unwrap_or(0),
        DSVFormat: match desc.depth_stencil {
            Some(state) => conv::dxgi_format(state.format),
            None => DXGI_FORMAT_UNKNOWN,
        },
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: samples,
            // Quality zero is the standard pattern; the seam has no vocabulary
            // for a vendor's extended patterns and inventing one here would be a
            // guess about what a caller asked for.
            Quality: 0,
        },
        NodeMask: 0,
        Flags: D3D12_PIPELINE_STATE_FLAG_NONE,
        ..Default::default()
    };
    for (index, target) in desc.color_targets.iter().enumerate() {
        state.RTVFormats[index] = conv::dxgi_format(target.format);
    }

    // SAFETY: `state` is a live, fully initialised descriptor borrowed for the
    // call. Its `VS`/`PS` pointers borrow `vertex` and `fragment`, which outlive
    // this statement, and `pRootSignature` holds a reference released just
    // below. `ID3D12PipelineState` is the IID asked for.
    let created: Result<ID3D12PipelineState, _> =
        unsafe { device.CreateGraphicsPipelineState(&raw const state) };
    release_root_signature(&mut state);
    let raw = created.map_err(|error| {
        HalError::PipelineCreation(format!(
            "ID3D12Device::CreateGraphicsPipelineState rejected `{}`: {error}",
            desc.label.unwrap_or("<unlabelled>")
        ))
    })?;

    Ok(GraphicsPipelineEntry {
        owner,
        raw,
        root_signature: layout.raw.clone(),
        topology: conv::primitive_topology(desc.primitive.topology),
        stencil_reference: desc
            .depth_stencil
            .and_then(|state| state.stencil)
            .map(|stencil| stencil.reference),
    })
}

/// Releases the reference the pipeline descriptor's `pRootSignature` holds.
///
/// `D3D12_GRAPHICS_PIPELINE_STATE_DESC::pRootSignature` is a
/// `ManuallyDrop<Option<ID3D12RootSignature>>`, so a descriptor built with a
/// cloned interface owns a reference nothing would release. Every exit from
/// [`graphics`] after the descriptor exists goes through here.
fn release_root_signature(state: &mut D3D12_GRAPHICS_PIPELINE_STATE_DESC) {
    // SAFETY: `pRootSignature` was written exactly once, by the initialiser in
    // `graphics`, and this is its matching release. The field is not read again:
    // every caller returns or moves on immediately afterwards.
    unsafe { ManuallyDrop::drop(&mut state.pRootSignature) };
}

/// Builds a `D3D12_COMPUTE_PIPELINE_STATE_DESC` and the object from it.
///
/// Nearly all of the machinery is [`graphics`]': the same root signature, the
/// same validated DXIL container, the same `ManuallyDrop` discipline on
/// `pRootSignature`. What is absent is everything a rasteriser needs — there is
/// no blend, depth, sample or attachment state on a compute pipeline, and
/// nothing is left on the command list for [`bind_compute_pipeline`] to replay.
///
/// # The workgroup size is checked against the container, not merely the limits
///
/// [`ComputePipelineDesc::workgroup_size`] exists because MSL cannot declare a
/// thread count. DXIL can, and does: `[numthreads(x, y, z)]` is in the `PSV0`
/// part of every container `dxc` signs, so a descriptor that disagrees with the
/// artifact is caught here — the same check `crcbl-vk` makes from
/// `OpExecutionMode LocalSize`. See [`crate::dxil`] for how it is read and for
/// the arm where a container is too old to say.
///
/// [`bind_compute_pipeline`]: crcbl_hal::CommandEncoder::bind_compute_pipeline
///
/// # Errors
///
/// [`HalError::ShaderCompilation`] for a container that is not a compute shader
/// or whose thread-group size is not the one the descriptor declares, and
/// [`HalError::PipelineCreation`] carrying D3D12's own message when the driver
/// refuses the object — which is what a root signature that does not cover the
/// registers the shader reads arrives as.
pub(crate) fn compute(
    device: &ID3D12Device,
    desc: &ComputePipelineDesc<'_>,
    layout: &PipelineLayoutEntry,
    module: &ShaderModuleEntry,
    owner: u64,
) -> Result<ComputePipelineEntry, HalError> {
    let label = desc.label.unwrap_or("<unlabelled>");
    let dxil = module.container(desc.compute.entry_point)?;
    dxil.expect(ShaderStages::COMPUTE, desc.compute.entry_point)?;
    if let Some(declared) = dxil.numthreads()
        && declared != desc.workgroup_size
    {
        return Err(HalError::ShaderCompilation(format!(
            "`{label}` asks for a workgroup of {:?} and `{}`'s container declares \
             [numthreads({}, {}, {})]; the shader's own number is the one the dispatch runs, so \
             the descriptor would launch a different grid than it computed",
            desc.workgroup_size, desc.compute.entry_point, declared[0], declared[1], declared[2],
        )));
    }

    // As `graphics`: every fallible step happens before the descriptor exists,
    // so no `?` can leak the reference `pRootSignature` clones.
    let mut state = D3D12_COMPUTE_PIPELINE_STATE_DESC {
        pRootSignature: ManuallyDrop::new(Some(layout.raw.clone())),
        CS: dxil.bytecode(),
        NodeMask: 0,
        Flags: D3D12_PIPELINE_STATE_FLAG_NONE,
        ..Default::default()
    };
    // SAFETY: `state` is a live, fully initialised descriptor borrowed for the
    // call. Its `CS` pointer borrows `module`, which outlives this statement,
    // and `pRootSignature` holds a reference released just below.
    // `ID3D12PipelineState` is the IID asked for.
    let created: Result<ID3D12PipelineState, _> =
        unsafe { device.CreateComputePipelineState(&raw const state) };
    // SAFETY: `pRootSignature` was written exactly once, by the initialiser
    // above, and this is its matching release. The descriptor is not read again.
    unsafe { ManuallyDrop::drop(&mut state.pRootSignature) };
    let raw = created.map_err(|error| {
        HalError::PipelineCreation(format!(
            "ID3D12Device::CreateComputePipelineState rejected `{label}`: {error}"
        ))
    })?;

    Ok(ComputePipelineEntry {
        owner,
        raw,
        root_signature: layout.raw.clone(),
    })
}

/// Blend state for every declared colour target.
///
/// `IndependentBlendEnable` is always on, because the seam gives blend state per
/// target and D3D12 otherwise applies slot zero's to all of them — which would
/// silently blend an additive overlay with the opaque pass's state.
fn blend_state(targets: &[ColorTargetState], multisample: &MultisampleState) -> D3D12_BLEND_DESC {
    let mut blend = D3D12_BLEND_DESC {
        AlphaToCoverageEnable: multisample.alpha_to_coverage.into(),
        IndependentBlendEnable: true.into(),
        RenderTarget: [D3D12_RENDER_TARGET_BLEND_DESC::default(); RENDER_TARGETS],
    };
    for (slot, target) in blend.RenderTarget.iter_mut().zip(targets) {
        slot.RenderTargetWriteMask = conv::color_write_mask(target.write_mask);
        slot.LogicOpEnable = false.into();
        // A zeroed `D3D12_LOGIC_OP` is `CLEAR`, and the debug layer objects to
        // it even with `LogicOpEnable` off. `NOOP` is the neutral one.
        slot.LogicOp = D3D12_LOGIC_OP_NOOP;
        let Some(state) = target.blend else {
            continue;
        };
        slot.BlendEnable = true.into();
        slot.SrcBlend = conv::blend_factor(state.color_src);
        slot.DestBlend = conv::blend_factor(state.color_dst);
        slot.BlendOp = conv::blend_op(state.color_op);
        slot.SrcBlendAlpha = conv::blend_factor(state.alpha_src);
        slot.DestBlendAlpha = conv::blend_factor(state.alpha_dst);
        slot.BlendOpAlpha = conv::blend_op(state.alpha_op);
    }
    blend
}

/// Rasteriser state, including the depth bias D3D12 keeps here rather than with
/// the depth test.
///
/// **`DepthBias` is an integer in D3D12 and an `f32` in the seam**, because
/// D3D12 defines a constant bias in units of the depth buffer's smallest
/// resolvable difference — which is what `crcbl_hal::DepthBias::constant`
/// documents it as too. Truncating rather than rounding keeps a bias no larger
/// than the caller asked for, in whichever direction reversed-Z put the sign.
fn rasterizer_state(
    primitive: &PrimitiveState,
    depth: Option<&DepthStencilState>,
    samples: u32,
) -> D3D12_RASTERIZER_DESC {
    let bias = depth.map_or_else(Default::default, |state| state.bias);
    #[allow(clippy::cast_possible_truncation)]
    let constant = bias.constant.trunc() as i32;
    D3D12_RASTERIZER_DESC {
        FillMode: conv::fill_mode(primitive.polygon_mode),
        CullMode: conv::cull_mode(primitive.cull_mode),
        // The seam's `FrontFace::Ccw` is counter-clockwise in framebuffer space,
        // which is exactly what this flag names.
        FrontCounterClockwise: matches!(primitive.front_face, crcbl_hal::FrontFace::Ccw).into(),
        DepthBias: constant,
        DepthBiasClamp: bias.clamp,
        SlopeScaledDepthBias: bias.slope_scale,
        // The seam's `depth_clamp` asks for clamping *instead of* clipping, so
        // it is this flag inverted rather than a flag of its own.
        DepthClipEnable: (!primitive.depth_clamp).into(),
        MultisampleEnable: (samples > 1).into(),
        AntialiasedLineEnable: false.into(),
        // Zero means "the sample count comes from the render target", which is
        // the only reading that agrees with `SampleDesc` above.
        ForcedSampleCount: 0,
        ConservativeRaster: D3D12_CONSERVATIVE_RASTERIZATION_MODE_OFF,
    }
}

/// Depth and stencil state.
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`] when the state's format has no depth plane,
/// or declares a stencil test on a format with no stencil plane — the second
/// being the one that otherwise passes creation and tests against a plane that
/// does not exist.
fn depth_stencil_state(
    state: Option<&DepthStencilState>,
) -> Result<D3D12_DEPTH_STENCIL_DESC, HalError> {
    let Some(state) = state else {
        // Zeroed is "no depth test, no depth write", which is what a pipeline
        // with no depth attachment means. `DepthFunc` must still be a legal
        // value: the debug layer reads a zero as `D3D12_COMPARISON_FUNC_NONE`.
        return Ok(D3D12_DEPTH_STENCIL_DESC {
            DepthFunc: D3D12_COMPARISON_FUNC_ALWAYS,
            ..Default::default()
        });
    };
    if !state.format.is_depth_stencil() {
        return Err(HalError::InvalidDescriptor(format!(
            "DepthStencilState::format is {:?}, which has no depth plane",
            state.format
        )));
    }
    if state.stencil.is_some() && !state.format.has_stencil() {
        return Err(HalError::InvalidDescriptor(format!(
            "a stencil state on a {:?} pipeline, which has no stencil plane to test",
            state.format
        )));
    }
    let faces = state.stencil.map(|stencil| {
        let face = |side: crcbl_hal::StencilFaceState| D3D12_DEPTH_STENCILOP_DESC {
            StencilFailOp: conv::stencil_op(side.fail_op),
            StencilDepthFailOp: conv::stencil_op(side.depth_fail_op),
            StencilPassOp: conv::stencil_op(side.pass_op),
            StencilFunc: conv::comparison_func(side.compare),
        };
        (stencil, face(stencil.front), face(stencil.back))
    });
    Ok(D3D12_DEPTH_STENCIL_DESC {
        DepthEnable: true.into(),
        DepthWriteMask: if state.depth_write {
            D3D12_DEPTH_WRITE_MASK_ALL
        } else {
            D3D12_DEPTH_WRITE_MASK_ZERO
        },
        // No reversed-Z flip; see the module docs.
        DepthFunc: conv::comparison_func(state.depth_compare),
        StencilEnable: faces.is_some().into(),
        // D3D12's masks are 8-bit where the seam's are 32; a mask that does not
        // fit is truncated to the plane that exists, which is every bit D3D12
        // has.
        #[allow(clippy::cast_possible_truncation)]
        StencilReadMask: faces.map_or(0, |(stencil, ..)| stencil.read_mask as u8),
        #[allow(clippy::cast_possible_truncation)]
        StencilWriteMask: faces.map_or(0, |(stencil, ..)| stencil.write_mask as u8),
        FrontFace: faces.map_or_else(Default::default, |(_, front, _)| front),
        BackFace: faces.map_or_else(Default::default, |(_, _, back)| back),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_hal::{BlendState, ColorWrites, CompareOp, Format, StencilState};

    /// Blend state is per target, and a target with no blend still writes the
    /// channels it asked for.
    ///
    /// `IndependentBlendEnable` is the assertion that matters: with it off,
    /// D3D12 applies slot zero's state to every target, so an additive overlay
    /// beside an opaque pass would blend with the opaque pass's state and look
    /// almost right.
    #[test]
    fn blend_state_is_per_target_and_independent() {
        let targets = [
            ColorTargetState::opaque(Format::Rgba16Float),
            ColorTargetState {
                format: Format::Rgba8Unorm,
                blend: Some(BlendState::additive()),
                write_mask: ColorWrites::R | ColorWrites::A,
            },
        ];
        let blend = blend_state(&targets, &MultisampleState::default());
        assert!(blend.IndependentBlendEnable.as_bool());
        assert!(!blend.AlphaToCoverageEnable.as_bool());
        assert!(!blend.RenderTarget[0].BlendEnable.as_bool());
        assert_eq!(blend.RenderTarget[0].RenderTargetWriteMask, 0b1111);
        assert!(blend.RenderTarget[1].BlendEnable.as_bool());
        assert_eq!(
            blend.RenderTarget[1].RenderTargetWriteMask, 0b1001,
            "red and alpha, and D3D12's bit order is R=1 G=2 B=4 A=8"
        );
        assert!(
            !blend.RenderTarget[2].BlendEnable.as_bool(),
            "a slot no target declared must stay off"
        );
    }

    /// Reversed-Z arrives unchanged, and depth writes follow the descriptor.
    #[test]
    fn depth_state_carries_the_engines_reversed_z_through_unchanged() {
        let state = depth_stencil_state(Some(&DepthStencilState::default()))
            .expect("the seam's own default");
        assert!(state.DepthEnable.as_bool());
        assert_eq!(state.DepthWriteMask, D3D12_DEPTH_WRITE_MASK_ALL);
        assert_eq!(
            state.DepthFunc,
            conv::comparison_func(CompareOp::Greater),
            "a backend that flipped the sense would invert every depth test"
        );
        assert!(!state.StencilEnable.as_bool());

        let read_only = depth_stencil_state(Some(&DepthStencilState::equal_depth_read_only(
            Format::D32Float,
        )))
        .expect("a prepass-matching state");
        assert_eq!(read_only.DepthWriteMask, D3D12_DEPTH_WRITE_MASK_ZERO);
        assert_eq!(
            read_only.DepthFunc,
            conv::comparison_func(CompareOp::GreaterOrEqual)
        );

        // And no depth state at all is "always pass, never write" rather than a
        // zeroed `DepthFunc`, which the debug layer reads as a value no
        // comparison may take.
        let none = depth_stencil_state(None).expect("a pipeline with no depth attachment");
        assert!(!none.DepthEnable.as_bool());
        assert_eq!(none.DepthFunc, D3D12_COMPARISON_FUNC_ALWAYS);
    }

    /// A depth/stencil state D3D12 cannot express is refused by name.
    #[test]
    fn a_depth_state_without_the_plane_it_tests_is_refused() {
        let error = depth_stencil_state(Some(&DepthStencilState {
            format: Format::Rgba8Unorm,
            ..DepthStencilState::default()
        }))
        .expect_err("a colour format has no depth plane");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

        let error = depth_stencil_state(Some(&DepthStencilState {
            format: Format::D32Float,
            stencil: Some(StencilState::default()),
            ..DepthStencilState::default()
        }))
        .expect_err("D32Float has no stencil plane");
        let HalError::InvalidDescriptor(text) = &error else {
            panic!("{error:?}");
        };
        assert!(text.contains("no stencil plane"), "{text}");

        // A format that *does* have both takes the same state, so the refusal
        // above is the plane check rather than a blanket refusal of stencil.
        let both = depth_stencil_state(Some(&DepthStencilState {
            format: Format::D24UnormS8Uint,
            stencil: Some(StencilState {
                read_mask: 0xFF,
                write_mask: 0x0F,
                ..StencilState::default()
            }),
            ..DepthStencilState::default()
        }))
        .expect("D24_UNORM_S8_UINT has both planes");
        assert!(both.StencilEnable.as_bool());
        assert_eq!(both.StencilReadMask, 0xFF);
        assert_eq!(both.StencilWriteMask, 0x0F);
    }

    /// The rasteriser reads the seam's winding and clamp the way round D3D12
    /// spells them — both are inverted relative to the seam's wording, and both
    /// would produce a picture that is merely wrong rather than broken.
    #[test]
    fn the_rasteriser_inverts_neither_the_winding_nor_the_clamp() {
        let raster = rasterizer_state(&PrimitiveState::default(), None, 1);
        assert!(
            raster.FrontCounterClockwise.as_bool(),
            "the seam's default front face is counter-clockwise"
        );
        assert!(
            raster.DepthClipEnable.as_bool(),
            "depth_clamp is off by default, so clipping is on"
        );
        assert!(!raster.MultisampleEnable.as_bool());

        let clamped = rasterizer_state(
            &PrimitiveState {
                front_face: crcbl_hal::FrontFace::Cw,
                depth_clamp: true,
                ..PrimitiveState::default()
            },
            None,
            4,
        );
        assert!(!clamped.FrontCounterClockwise.as_bool());
        assert!(!clamped.DepthClipEnable.as_bool());
        assert!(clamped.MultisampleEnable.as_bool());
    }
}
