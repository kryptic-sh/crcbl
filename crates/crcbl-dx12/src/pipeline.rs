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
//! Each bind group layout becomes one or two descriptor tables — a CBV/SRV/UAV
//! table and a sampler table, which D3D12 will not let share one — plus a **root
//! descriptor per dynamic binding**, and `crcbl_dx12::binding` computes what
//! each contains. Which root parameter index each landed at is
//! [`crate::root`]'s answer, because
//! [`bind_group`](crcbl_hal::CommandEncoder::bind_group) is given a set index
//! while `SetGraphicsRootDescriptorTable` and
//! `SetGraphicsRootConstantBufferView` take a root parameter index, and the two
//! are the same number only when every set is exactly one table.
//!
//! **This module does not decide that order, it replays it.** [`plan_root`]
//! iterates [`root::RootLayout::slots`] — the parameter array's own order, as
//! `crate::root` assigned it — so the indices `crate::device` binds against
//! cannot drift from the signature these parameters describe. The same call is
//! what refuses a layout costing more than D3D12's 64 root DWORDs, at layout
//! creation rather than at the draw.
//!
//! Root signature **1.0** is serialised, deliberately: its descriptor ranges are
//! volatile by definition, which is what
//! [`BindingFlags::UPDATE_AFTER_BIND`](crcbl_hal::BindingFlags::UPDATE_AFTER_BIND)
//! asks for, and 1.1 would add a `CheckFeatureSupport` call and a fallback path
//! to buy static-descriptor optimisations nothing here measures.
//!
//! A **push-constant range is a root-constants parameter**, which D3D12 spells
//! `D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS`: a `D3D12_ROOT_CONSTANTS` naming
//! a shader register and a count of 32-bit values stored in the signature
//! itself. [`crate::root`] decides its register, its word count and its place
//! among the parameters — see that module on why the register is the one after
//! every binding's, and on why the words come out of the same 64-DWORD budget
//! the tables do.
//!
//! There is no `pDescriptorRanges` to keep alive for one, so unlike a table it
//! owns no allocation: the register and the count *are* the parameter.
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
//! replays it. The stencil reference is *not* a second one: D3D12 does keep it
//! on the command list, but the seam has no pipeline field to carry — it is
//! pass state, set only by
//! [`set_stencil_reference`](crcbl_hal::CommandEncoder::set_stencil_reference)
//! — so a bind must leave the list's current value alone.
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
    BackendKind, ColorTargetState, ComputePipelineDesc, DepthStencilState, GraphicsPipelineDesc,
    HalError, MeshPipelineDesc, MultisampleState, PipelineLayoutDesc, PrimitiveState, ShaderStages,
};
use windows::Win32::Graphics::Direct3D::{D3D_PRIMITIVE_TOPOLOGY, ID3DBlob};
use windows::Win32::Graphics::Direct3D12::{
    D3D_ROOT_SIGNATURE_VERSION_1, D3D12_BLEND_DESC, D3D12_COMPARISON_FUNC_ALWAYS,
    D3D12_COMPUTE_PIPELINE_STATE_DESC, D3D12_CONSERVATIVE_RASTERIZATION_MODE_OFF,
    D3D12_DEPTH_STENCIL_DESC, D3D12_DEPTH_STENCILOP_DESC, D3D12_DEPTH_WRITE_MASK_ALL,
    D3D12_DEPTH_WRITE_MASK_ZERO, D3D12_DESCRIPTOR_RANGE, D3D12_GRAPHICS_PIPELINE_STATE_DESC,
    D3D12_INDEX_BUFFER_STRIP_CUT_VALUE_DISABLED, D3D12_INPUT_LAYOUT_DESC, D3D12_LOGIC_OP_NOOP,
    D3D12_MAX_ROOT_COST, D3D12_PIPELINE_STATE_FLAG_NONE, D3D12_PIPELINE_STATE_FLAGS,
    D3D12_PIPELINE_STATE_STREAM_DESC, D3D12_PIPELINE_STATE_SUBOBJECT_TYPE,
    D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_AS, D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_BLEND,
    D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL,
    D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL_FORMAT,
    D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_FLAGS, D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_MS,
    D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PS, D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_RASTERIZER,
    D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_RENDER_TARGET_FORMATS,
    D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_ROOT_SIGNATURE,
    D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_SAMPLE_DESC,
    D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_SAMPLE_MASK, D3D12_RASTERIZER_DESC,
    D3D12_RENDER_TARGET_BLEND_DESC, D3D12_ROOT_CONSTANTS, D3D12_ROOT_DESCRIPTOR_TABLE,
    D3D12_ROOT_PARAMETER, D3D12_ROOT_PARAMETER_0, D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS,
    D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE, D3D12_ROOT_SIGNATURE_DESC,
    D3D12_ROOT_SIGNATURE_FLAG_NONE, D3D12_RT_FORMAT_ARRAY, D3D12_SHADER_BYTECODE,
    D3D12SerializeRootSignature, ID3D12Device, ID3D12Device2, ID3D12PipelineState,
    ID3D12RootSignature,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC};
use windows::core::Interface;

use crate::binding::SetTables;
use crate::conv;
use crate::dxil::{Dxil, ShaderModuleEntry};
use crate::handle::Owned;
use crate::root::{self, RootConstants, SetPlacement, Slot, SlotKind};
use crate::stream::Stream;

/// `crate::root` spells D3D12's root budget out because it compiles off Windows.
/// This is the build that has the real constant, so this is where the two are
/// made to agree — a divergence would let a layout D3D12 refuses pass the check
/// meant to catch it.
const _: () = assert!(root::MAX_ROOT_COST == D3D12_MAX_ROOT_COST);

/// Colour attachments a D3D12 pipeline state object can declare.
///
/// The array's own length in `D3D12_GRAPHICS_PIPELINE_STATE_DESC`, which is what
/// makes a longer `color_targets` a refusal rather than a silent truncation.
const RENDER_TARGETS: usize = 8;

/// Every sample covered, which is what `SampleMask` is on every pipeline here.
///
/// [`MultisampleState`] carries no mask: `MTLRenderPipelineDescriptor` has no
/// member to put one in, so the seam guarantees full coverage instead of a
/// field one backend would have to refuse. D3D12 has no "all bits" sentinel —
/// `SampleMask` of `0` really does discard every sample — so the guarantee is
/// spelled out here rather than left to a struct field's default.
const ALL_SAMPLES: u32 = u32::MAX;

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
    /// Where the root constants are and what range they cover, or `None` for a
    /// layout that declared no push-constant range. This is what
    /// [`push_constants`](crcbl_hal::CommandEncoder::push_constants) is checked
    /// against — see [`root::write`].
    pub(crate) push_constants: Option<root::Declared>,
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
    /// `IASetPrimitiveTopology`, which the PSO takes only the category of — or
    /// `None` for a **mesh** pipeline, which has no input assembler to set one
    /// on. The topology there is the mesh shader's own `[outputtopology(…)]`,
    /// and `IASetPrimitiveTopology(UNDEFINED)` is a debug-layer error rather
    /// than a no-op, so the absence has to be a value the encoder can read.
    pub(crate) topology: Option<D3D_PRIMITIVE_TOPOLOGY>,
}

/// A compute pipeline: the state object and the signature it was built against.
///
/// No topology, which is the whole difference from [`GraphicsPipelineEntry`]:
/// D3D12 leaves nothing about a dispatch on the command list except the root
/// arguments every pipeline needs.
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

/// A whole signature's root parameters, and the ranges they point at.
///
/// The ranges are returned alongside because `D3D12_ROOT_DESCRIPTOR_TABLE` holds
/// a *pointer* into them: they must outlive the serialisation call, and keeping
/// them in one owned `Vec` per table is what guarantees that without a lifetime
/// nobody can express in an FFI struct.
struct RootSignaturePlan {
    parameters: Vec<D3D12_ROOT_PARAMETER>,
    /// One entry per set, in set order.
    sets: Vec<SetPlacement>,
    /// Where the root constants landed, if the layout declares a range.
    push_constants: Option<root::Declared>,
    /// Kept alive for the duration of the serialise call; never read again.
    _ranges: Vec<Vec<D3D12_DESCRIPTOR_RANGE>>,
}

/// Builds a root signature from a pipeline layout's sets and its push-constant
/// range.
///
/// `push` is the range already planned by
/// [`root::plan_push_constants`], because the shader register it takes comes
/// from the same counter the sets' registers do and `crate::device` is where
/// that counter lives.
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`] for more sets than
/// [`Limits::max_bind_groups`](crcbl_hal::Limits::max_bind_groups), or a
/// signature costing more root DWORDs than D3D12 holds — see
/// [`root::place`] — and [`HalError::Backend`] when D3D12 refuses to serialise
/// or create the signature, carrying the serialiser's own error blob, which is
/// the only text that says *which* parameter it objected to.
pub(crate) fn layout(
    device: &ID3D12Device,
    desc: &PipelineLayoutDesc<'_>,
    sets: &[(crcbl_hal::BindGroupLayoutHandle, SetTables)],
    push: Option<RootConstants>,
    owner: u64,
) -> Result<PipelineLayoutEntry, HalError> {
    let plan = plan_root(sets, push)?;
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
        push_constants: plan.push_constants,
    })
}

/// Builds the root parameter array, in the order [`root::place`] laid it out.
///
/// **The loop is over `layout.slots`, not over `sets`**, and that is the whole
/// point: the indices `crate::device` binds against come from
/// [`root::RootLayout::sets`], which was filled in the same pass. A second
/// ordering rule written here is the bug this shape rules out.
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`] when the signature does not fit D3D12's root
/// budget. See [`root::place`].
fn plan_root(
    sets: &[(crcbl_hal::BindGroupLayoutHandle, SetTables)],
    push: Option<RootConstants>,
) -> Result<RootSignaturePlan, HalError> {
    let shapes: Vec<root::SetShape> = sets
        .iter()
        .map(|(_, tables)| root::SetShape {
            views: !tables.views.is_empty(),
            samplers: !tables.samplers.is_empty(),
            // A layout's entries come from a slice, so this cannot exceed a
            // `u32` on any target this crate builds for; saturating keeps the
            // budget check on the right side either way.
            roots: u32::try_from(tables.roots.len()).unwrap_or(u32::MAX),
        })
        .collect();
    let layout = root::place(&shapes, push)?;

    let mut parameters = Vec::with_capacity(layout.slots.len());
    let mut owned: Vec<Vec<D3D12_DESCRIPTOR_RANGE>> = Vec::new();
    for slot in &layout.slots {
        let Slot::Set { set, kind } = *slot else {
            let constants =
                push.unwrap_or_else(|| unreachable!("a constants slot is placed with a range"));
            parameters.push(D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS,
                // As a root descriptor: the register and the count are the
                // parameter, so there is nothing to keep alive past the call.
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    Constants: D3D12_ROOT_CONSTANTS {
                        ShaderRegister: constants.register,
                        RegisterSpace: 0,
                        Num32BitValues: constants.words,
                    },
                },
                ShaderVisibility: conv::shader_visibility(constants.stages),
            });
            continue;
        };
        let tables = &sets
            .get(set)
            .unwrap_or_else(|| unreachable!("every slot names a set that was placed"))
            .1;
        match kind {
            SlotKind::Views | SlotKind::Samplers => {
                let ranges = if kind == SlotKind::Views {
                    &tables.views
                } else {
                    &tables.samplers
                };
                // Cloned into `owned` and pointed at from there. Pushing another
                // entry later moves the outer `Vec`'s elements but never the
                // inner allocation this pointer names, which is what makes
                // taking the address here sound while the list is still being
                // built.
                owned.push(ranges.clone());
                let held = owned.last().expect("just pushed");
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
            SlotKind::Root(index) => {
                let root = tables.roots.get(index).unwrap_or_else(|| {
                    unreachable!("the shape counted this set's root descriptors")
                });
                parameters.push(D3D12_ROOT_PARAMETER {
                    ParameterType: root.parameter_type,
                    // A root descriptor holds the register itself rather than a
                    // pointer to anything, so it has no owned allocation to keep
                    // alive the way a table's ranges do.
                    Anonymous: D3D12_ROOT_PARAMETER_0 {
                        Descriptor: root.descriptor,
                    },
                    // Its own stage rather than the set's union: one root
                    // parameter is one binding here, so nothing has to be
                    // widened to cover a neighbour.
                    ShaderVisibility: conv::shader_visibility(root.visibility),
                });
            }
        }
    }

    Ok(RootSignaturePlan {
        parameters,
        sets: layout.sets,
        // Both halves come from the one `place` pass: the parameter index it
        // assigned, and the range that index's parameter was sized from.
        push_constants: layout
            .push_constants
            .zip(push)
            .map(|(parameter, constants)| root::Declared {
                parameter,
                offset: constants.range.offset,
                size: constants.range.size,
            }),
        _ranges: owned,
    })
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
        SampleMask: ALL_SAMPLES,
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
        topology: Some(conv::primitive_topology(desc.primitive.topology)),
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

/// A *permanent* refusal on a Windows old enough to have no stream path, and
/// deliberately not an unwritten slice's voice: no slice of work in this
/// repository makes `ID3D12Device2` appear on a runtime that predates it.
///
/// It is separately worth naming because the interface arrived in Windows 10
/// 1703 while the mesh stages themselves arrived in 2004, so a device that fails
/// this cast has no mesh tier either — which is the answer
/// `crcbl_dx12::adapter`'s `features_of` would have given first if this backend
/// reported the flag.
const NO_PIPELINE_STATE_STREAM: &str = "a mesh pipeline: D3D12_GRAPHICS_PIPELINE_STATE_DESC has no slot for an amplification or mesh \
     stage, so the only call that takes one is ID3D12Device2::CreatePipelineState — and this \
     device does not implement ID3D12Device2";

/// One subobject of a pipeline state stream, paired with the tag D3D12 reads it
/// as.
///
/// `CreatePipelineState` walks a byte array in which every subobject is a `u32`
/// tag followed by the payload that tag names, so the pairing is the whole
/// contract — see [`crate::stream`] for the packing it implies.
///
/// # Safety
///
/// `TAG` must be the `D3D12_PIPELINE_STATE_SUBOBJECT_TYPE` whose documented
/// payload is exactly `Self`, and `Self` must be the exact struct D3D12 defines
/// for it. The runtime reads `size_of::<Self>()` bytes and interprets them per
/// the tag, so a mismatched pair is one struct read as another — a shader
/// bytecode pointer taken out of the middle of a blend state, with no
/// diagnostic anywhere.
///
/// This is the same hazard [`crate::adapter`]'s `FeatureQuery` exists for, and
/// it is enforced the same way: the tag comes from the type, so [`add`] cannot
/// be called with a pair nobody wrote down.
unsafe trait Subobject: Copy {
    /// The tag D3D12 reads this payload under.
    const TAG: D3D12_PIPELINE_STATE_SUBOBJECT_TYPE;
}

/// The root signature subobject's payload: the interface **pointer itself**,
/// not a pointer to it.
///
/// A borrow rather than a reference — `as_raw` takes no refcount — so unlike
/// `D3D12_GRAPHICS_PIPELINE_STATE_DESC::pRootSignature` there is nothing here
/// for [`release_root_signature`] to give back. What replaces that discipline
/// is the obligation stated on [`mesh`]: the layout outlives the call.
#[repr(transparent)]
#[derive(Clone, Copy)]
struct RootSignature(*mut core::ffi::c_void);

/// The mesh stage's bytecode. A newtype because three stages share
/// `D3D12_SHADER_BYTECODE` and each is its own tag, which is exactly the pairing
/// [`Subobject`] exists to fix.
#[repr(transparent)]
#[derive(Clone, Copy)]
struct MeshShader(D3D12_SHADER_BYTECODE);

/// The amplification stage's bytecode — the seam's `task` stage.
#[repr(transparent)]
#[derive(Clone, Copy)]
struct TaskShader(D3D12_SHADER_BYTECODE);

/// The pixel stage's bytecode.
#[repr(transparent)]
#[derive(Clone, Copy)]
struct PixelShader(D3D12_SHADER_BYTECODE);

/// The sample mask, which is a bare `u32` in the stream and would otherwise be
/// indistinguishable from any other.
#[repr(transparent)]
#[derive(Clone, Copy)]
struct SampleMask(u32);

/// The root signature subobject's payload is the **interface pointer**, so the
/// newtype has to be laid out exactly as `windows` lays that interface out.
///
/// A `const` assertion rather than a sentence, because it is the one pairing
/// above whose payload is not a `windows` struct named after its tag —
/// everything else is checked by the type system when the `impl` names it.
const _: () = assert!(size_of::<RootSignature>() == size_of::<ID3D12RootSignature>());
const _: () = assert!(align_of::<RootSignature>() == align_of::<ID3D12RootSignature>());

// SAFETY for each: the payload is the struct D3D12 documents for the tag beside
// it, taken from `windows`' own bindings rather than redeclared here. Every
// newtype above is `repr(transparent)` over exactly that payload — a
// `D3D12_SHADER_BYTECODE` for the three stages, D3D12's own `UINT` for the
// sample mask, and the interface pointer the assertions above pin for the root
// signature — so each has its size and alignment.
unsafe impl Subobject for RootSignature {
    const TAG: D3D12_PIPELINE_STATE_SUBOBJECT_TYPE =
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_ROOT_SIGNATURE;
}
unsafe impl Subobject for MeshShader {
    const TAG: D3D12_PIPELINE_STATE_SUBOBJECT_TYPE = D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_MS;
}
unsafe impl Subobject for TaskShader {
    const TAG: D3D12_PIPELINE_STATE_SUBOBJECT_TYPE = D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_AS;
}
unsafe impl Subobject for PixelShader {
    const TAG: D3D12_PIPELINE_STATE_SUBOBJECT_TYPE = D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PS;
}
unsafe impl Subobject for SampleMask {
    const TAG: D3D12_PIPELINE_STATE_SUBOBJECT_TYPE =
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_SAMPLE_MASK;
}
unsafe impl Subobject for D3D12_BLEND_DESC {
    const TAG: D3D12_PIPELINE_STATE_SUBOBJECT_TYPE = D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_BLEND;
}
unsafe impl Subobject for D3D12_RASTERIZER_DESC {
    const TAG: D3D12_PIPELINE_STATE_SUBOBJECT_TYPE = D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_RASTERIZER;
}
unsafe impl Subobject for D3D12_DEPTH_STENCIL_DESC {
    const TAG: D3D12_PIPELINE_STATE_SUBOBJECT_TYPE =
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL;
}
unsafe impl Subobject for D3D12_RT_FORMAT_ARRAY {
    const TAG: D3D12_PIPELINE_STATE_SUBOBJECT_TYPE =
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_RENDER_TARGET_FORMATS;
}
unsafe impl Subobject for DXGI_FORMAT {
    const TAG: D3D12_PIPELINE_STATE_SUBOBJECT_TYPE =
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL_FORMAT;
}
unsafe impl Subobject for DXGI_SAMPLE_DESC {
    const TAG: D3D12_PIPELINE_STATE_SUBOBJECT_TYPE =
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_SAMPLE_DESC;
}
unsafe impl Subobject for D3D12_PIPELINE_STATE_FLAGS {
    const TAG: D3D12_PIPELINE_STATE_SUBOBJECT_TYPE = D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_FLAGS;
}

/// Packs one subobject into `stream`.
///
/// The arithmetic is [`Stream::push`]'s, so the only thing happening here is the
/// copy of the payload into the space it reserved.
fn add<T: Subobject>(stream: &mut Stream, object: T) {
    let at = stream.push(T::TAG.0.cast_unsigned(), align_of::<T>(), size_of::<T>());
    let slot = stream.data_mut(at, size_of::<T>());
    // SAFETY: `push` reserved exactly `size_of::<T>()` bytes at `at`, so `slot`
    // is that long and the write lands wholly inside it. `write_unaligned`
    // rather than `write` because a `Vec<u8>`'s allocation is only byte-aligned,
    // and the stream's alignments are relative to its own start rather than to
    // an address — see `crate::stream`. `T: Copy`, so nothing is being moved out
    // of and the stream owns no destructor for what it now holds.
    unsafe { slot.as_mut_ptr().cast::<T>().write_unaligned(object) };
}

/// Builds a `D3D12_PIPELINE_STATE_STREAM_DESC` for a **mesh** pipeline and the
/// object from it.
///
/// # A mesh pipeline is the one graphics object `CreateGraphicsPipelineState`
/// cannot make
///
/// `D3D12_GRAPHICS_PIPELINE_STATE_DESC` has a `VS`, `PS`, `DS`, `HS` and `GS`
/// and no slot for either stage this builds, so the amplification and mesh
/// stages are reachable only through `ID3D12Device2::CreatePipelineState` and
/// the packed subobject stream [`crate::stream`] lays out. Everything the
/// subobjects *contain* is [`graphics`]': the same [`blend_state`],
/// [`rasterizer_state`] and [`depth_stencil_state`], from the same seam
/// descriptor fields.
///
/// # What is deliberately not in the stream
///
/// **No input layout, no index-buffer strip cut and no stream output**, because
/// a mesh pipeline has no input assembler at all — the mesh stage writes its own
/// vertices and index triples. `crcbl_hal::MeshPipelineDesc` says the same thing
/// one level up.
///
/// **No primitive topology.** `PrimitiveState::topology` is documented as
/// ignored on a mesh pipeline: the topology is the mesh shader's own
/// `[outputtopology(…)]`, and there are no input primitives to assemble. So
/// [`GraphicsPipelineEntry::topology`] is `None` for one of these and
/// `bind_graphics_pipeline` records no `IASetPrimitiveTopology`.
///
/// # Errors
///
/// [`HalError::Unsupported`] on a Windows runtime with no `ID3D12Device2`,
/// which is where the stream path lives.
/// [`HalError::InvalidDescriptor`] for a descriptor D3D12 cannot express — the
/// same ones [`graphics`] refuses — [`HalError::ShaderCompilation`] for a
/// container that is not the stage it was bound as, and
/// [`HalError::PipelineCreation`] carrying D3D12's own message when the driver
/// refuses the object, which is what a malformed stream arrives as.
pub(crate) fn mesh(
    device: &ID3D12Device,
    desc: &MeshPipelineDesc<'_>,
    layout: &PipelineLayoutEntry,
    task: Option<&ShaderModuleEntry>,
    mesh: &ShaderModuleEntry,
    fragment: Option<&ShaderModuleEntry>,
    owner: u64,
) -> Result<GraphicsPipelineEntry, HalError> {
    let label = desc.label.unwrap_or("<unlabelled>");
    let device2: ID3D12Device2 = device.cast().map_err(|error| {
        // The `HRESULT` goes to the log rather than into the refusal, because
        // `HalError::Unsupported::what` is a `&'static str` — and because the
        // code an absent interface answers with says nothing the sentence does
        // not.
        crcbl_core::log::debug!("crcbl-dx12: this device is not an ID3D12Device2: {error}");
        HalError::Unsupported {
            backend: BackendKind::Dx12,
            what: NO_PIPELINE_STATE_STREAM,
        }
    })?;

    let mesh_dxil = mesh.container(desc.mesh.entry_point)?;
    mesh_dxil.expect(ShaderStages::MESH, desc.mesh.entry_point)?;
    let task_dxil = match (task, desc.task) {
        (Some(module), Some(entry)) => {
            let dxil = module.container(entry.entry_point)?;
            dxil.expect(ShaderStages::TASK, entry.entry_point)?;
            Some(dxil)
        }
        _ => None,
    };
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
    for (index, target) in desc.color_targets.iter().enumerate() {
        if target.format.is_depth_stencil() {
            return Err(HalError::InvalidDescriptor(format!(
                "colour target {index} is {:?}, which is a depth/stencil format",
                target.format
            )));
        }
    }
    let samples = desc.multisample.samples.max(1);
    let depth_stencil = depth_stencil_state(desc.depth_stencil.as_ref())?;
    let mut formats = D3D12_RT_FORMAT_ARRAY {
        RTFormats: [DXGI_FORMAT_UNKNOWN; RENDER_TARGETS],
        NumRenderTargets: u32::try_from(desc.color_targets.len()).unwrap_or(0),
    };
    for (slot, target) in formats.RTFormats.iter_mut().zip(desc.color_targets) {
        *slot = conv::dxgi_format(target.format);
    }

    let mut stream = Stream::new();
    add(&mut stream, D3D12_PIPELINE_STATE_FLAG_NONE);
    // The pointer rather than a clone; see `RootSignature`.
    add(&mut stream, RootSignature(layout.raw.as_raw()));
    if let Some(dxil) = task_dxil {
        add(&mut stream, TaskShader(dxil.bytecode()));
    }
    add(&mut stream, MeshShader(mesh_dxil.bytecode()));
    // **Always present, empty when there is no fragment stage**, which is what
    // the raster path above does with `unwrap_or_default()` and is not
    // symmetry for its own sake. Omitting the subobject leaves the pixel stage
    // to the stream's default, and on WARP a mesh pipeline built that way
    // removes the device: `ID3D12Resource::Map` fails with
    // `DXGI_ERROR_DEVICE_REMOVED` and DRED reports no breadcrumbs at all, with
    // nothing from the debug layer. Measured by
    // `a_depth_only_mesh_pipeline_draws_the_toy_triangle_on_this_device`, which
    // runs stages six colour-target probes pass on, against
    // `a_mesh_pipeline_with_a_fragment_stage_and_a_depth_attachment_draws_both`
    // and `a_depth_only_raster_pipeline_draws_the_triangle_into_depth`, which
    // both draw — so it is this subobject's absence rather than depth, the
    // shader or the mesh stage.
    add(
        &mut stream,
        PixelShader(fragment_dxil.map(Dxil::bytecode).unwrap_or_default()),
    );
    add(
        &mut stream,
        blend_state(desc.color_targets, &desc.multisample),
    );
    add(&mut stream, SampleMask(ALL_SAMPLES));
    add(
        &mut stream,
        rasterizer_state(&desc.primitive, desc.depth_stencil.as_ref(), samples),
    );
    add(&mut stream, depth_stencil);
    if let Some(state) = desc.depth_stencil {
        add(&mut stream, conv::dxgi_format(state.format));
    }
    add(&mut stream, formats);
    add(
        &mut stream,
        DXGI_SAMPLE_DESC {
            Count: samples,
            // Quality zero, for the reason `graphics` gives.
            Quality: 0,
        },
    );

    let bytes = stream.as_mut_slice();
    let stream_desc = D3D12_PIPELINE_STATE_STREAM_DESC {
        SizeInBytes: bytes.len(),
        pPipelineStateSubobjectStream: bytes.as_mut_ptr().cast(),
    };
    // SAFETY: `stream_desc` names `stream`'s own live allocation and its own
    // length, and both outlive the call. Every subobject in it was written by
    // `add`, so each is one of D3D12's own structs under the tag that names it.
    // Every stage's bytecode borrows `mesh`, `task` or `fragment`, which are
    // parameters and therefore outlive this statement, and the root signature
    // pointer borrows `layout` on the same terms. `ID3D12PipelineState` is the
    // IID asked for.
    let raw: ID3D12PipelineState = unsafe { device2.CreatePipelineState(&raw const stream_desc) }
        .map_err(|error| {
        HalError::PipelineCreation(format!(
            "ID3D12Device2::CreatePipelineState rejected the mesh pipeline `{label}`: {error}"
        ))
    })?;

    Ok(GraphicsPipelineEntry {
        owner,
        raw,
        root_signature: layout.raw.clone(),
        // A mesh pipeline has no input assembly to set a topology on; see the
        // doc comment.
        topology: None,
    })
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
