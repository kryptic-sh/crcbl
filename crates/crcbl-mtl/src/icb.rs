//! The indirect-command-buffer path: how a GPU-read draw count becomes Metal
//! draws.
//!
//! # The problem this module exists for
//!
//! `MTLRenderCommandEncoder` has no `countBuffer:` draw. Every draw it takes
//! reads its *arguments* from GPU memory and its *count* from the CPU, so
//! [`draw_indirect_count`](crcbl_hal::CommandEncoder::draw_indirect_count) —
//! whose whole point is that the CPU never learns the count — has no direct
//! encoding. Metal's one count-from-memory execution is
//! `executeCommandsInBuffer:` over an `MTLIndirectCommandBuffer`, and an ICB
//! holds *commands*, not arguments: something has to write them first.
//!
//! That something is a compute kernel, and it has to run **before** the render
//! encoder the seam calls the verb inside was opened. `crcbl_mtl::command`
//! records a render pass rather than encoding it straight through precisely so
//! that it can: [`Encode`] is queued onto the recording, and
//! `encode_render_pass` runs every queued encode on one
//! `MTLComputeCommandEncoder` and *then* opens the render encoder.
//!
//! # The shape, end to end
//!
//! 1. `draw_indirect_count` bounds the call with
//!    [`plan_indirect_count`](crate::draw::plan_indirect_count), creates an ICB
//!    of `max_draw_count` commands and the [`ExecutionRange`] buffer beside it,
//!    and queues an [`Encode`].
//! 2. At `end_render_pass` the queued encodes run: one kernel dispatch each,
//!    `max_draw_count` threads wide. Thread *i* reads the count out of the
//!    count buffer and either encodes argument structure *i* into command *i*
//!    or **resets** command *i*, which makes it a no-op. Every command in the
//!    ICB is therefore written by exactly one thread, which is why nothing here
//!    resets the buffer first — an unwritten ICB command is undefined, and none
//!    is left unwritten. Thread zero additionally writes the clamped count into
//!    the execution-range buffer.
//! 3. The render encoder opens, replays the pass, and reaches
//!    `executeCommandsInBuffer:indirectBuffer:indirectBufferOffset:`, which
//!    reads *how many* commands to execute out of that buffer. So the number of
//!    draws that happen is the number the GPU wrote, and the CPU never learns
//!    it — which is what the seam verb means.
//!
//! # Why the range is read from memory rather than passed as an `NSRange`
//!
//! `executeCommandsInBuffer:withRange:` would also produce the right *picture*,
//! because a `reset()` command is documented as a no-op and every command past
//! the count is reset. It was what this module first shipped, and the slice was
//! reverted after it hung CI's GPU.
//!
//! Taking the range from memory removes the one shape nothing else in this
//! crate covers. Both of `crcbl_mtl::device`'s ICB rungs encode command zero
//! and then execute command zero; **neither has ever executed a command that
//! was reset**, from the CPU or from a kernel. The reverted path executed one
//! whenever a bucket came out empty — `crcbl_render::draw_gen` writes that
//! bucket's count word as nought — and with the range in memory that case is a
//! `length` of zero and an execute that reaches nothing at all.
//!
//! That is a *reason to prefer this shape*, not a diagnosis. The fault report
//! localised nothing: it listed every encoder of the hung command buffer,
//! including all three of this module's dispatches, as `completed`. Whether an
//! empty bucket occurred in the frames that hung was never established, and
//! only a Mac can establish it. `docs/backlog.md` carries the rest.
//!
//! # Why the kernel is hand-written MSL
//!
//! Every other shader this backend runs is compiled from Slang by
//! `crcbl-shaders`. **Slang's Metal target has no `command_buffer` and no
//! `render_command`**, and a parameter of that shape is dropped rather than
//! diagnosed — a Slang-authored version of [`ENCODE_MSL`] would compile to a
//! kernel that encodes nothing and the draws would silently disappear. So the
//! source is a Rust string, compiled with `newLibraryWithSource:options:error:`
//! the same way `crcbl_mtl::pipeline` compiles the committed MSL artifacts.
//!
//! It is outside `tools/compile-shaders.sh` entirely rather than exempted from
//! it: that script walks `shaders/*.slang` and refuses orphans *inside*
//! `crates/crcbl-shaders/msl/`, and this source never lives there. The cost is
//! the honest one — there is no `.slang` to regenerate this from, and nothing
//! checks it against a source because there is none.
//!
//! # Why the ICB reaches the kernel through an argument buffer
//!
//! Because that is the only route Metal offers: `MTLComputeCommandEncoder` has
//! no `setIndirectCommandBuffer:`, so a `command_buffer` can only be a member
//! of a struct the kernel takes by reference. The CPU writes that struct by
//! storing [`MTLIndirectCommandBuffer::gpuResourceID`] — "the handle of the GPU
//! resource suitable for storing in an argument buffer" — into an ordinary
//! `MTLBuffer`. No `MTLArgumentEncoder` is involved, for the reason
//! `crcbl_mtl::binding`'s bindless table gives: a Metal 3 argument buffer is a
//! plain struct in device memory, and a single 8-byte handle at offset zero has
//! the same layout under the MSL 2.x `[[id(n)]]` rules and the Metal 3 ones.
//!
//! # What this path cannot express
//!
//! **A negative `baseVertex`.** `MTLDrawIndexedPrimitivesIndirectArguments`
//! declares the field `int32_t`, and MSL's `render_command::
//! draw_indexed_primitives` takes `uint base_vertex` — so the word travels
//! through unchanged and a negative one arrives as a very large positive
//! offset. The direct
//! [`draw_indexed_indirect`](crcbl_hal::CommandEncoder::draw_indexed_indirect)
//! path does not have this limitation, because Metal reads the structure
//! itself there. `crcbl_shaders::draw_gen` writes only non-negative base
//! vertices, so nothing in this engine reaches it; a caller that needs one must
//! use the CPU-counted path.

use crcbl_hal::{HalError, MemoryLocation};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::{NSString, NSUInteger};
use objc2_metal::{
    MTLBlitCommandEncoder, MTLBuffer, MTLComputeCommandEncoder, MTLComputePipelineState, MTLDevice,
    MTLIndirectCommandBuffer, MTLIndirectCommandBufferDescriptor,
    MTLIndirectCommandBufferExecutionRange, MTLIndirectCommandType, MTLLibrary, MTLPrimitiveType,
    MTLResource, MTLResourceID, MTLResourceUsage, MTLSize,
};

use objc2_foundation::NSRange;

use crate::conv;
use crate::device::to_ns;

/// An `MTLBuffer` this path holds alive from the seam call to the submission.
type Buffer = Retained<ProtocolObject<dyn MTLBuffer>>;
/// The buffer of commands a kernel writes and a render encoder executes.
type Icb = Retained<ProtocolObject<dyn MTLIndirectCommandBuffer>>;
/// One specialised entry point of [`ENCODE_MSL`].
type Kernel = Retained<ProtocolObject<dyn MTLComputePipelineState>>;

/// The layout `executeCommandsInBuffer:indirectBuffer:indirectBufferOffset:`
/// reads its range in, named here because *two* compilers have to agree on it:
/// `ENCODE_MSL`'s `CrcblExecutionRange` is the same two words, written by the
/// kernel and read by Metal's command processor.
///
/// Nothing constructs one in Rust — the GPU is the only writer — so this alias
/// exists for [`RANGE_BYTES`] and for the assertion below it, which is what
/// makes the agreement a compile failure rather than a comment.
type ExecutionRange = MTLIndirectCommandBufferExecutionRange;

/// Bytes of the execution-range buffer each [`Encode`] allocates.
const RANGE_BYTES: u64 = size_of::<ExecutionRange>() as u64;

// `MTLIndirectCommandBufferExecutionRange` is `{ uint32_t location; uint32_t
// length; }` in Metal's own header, and `ENCODE_MSL` writes those two words in
// that order. A binding upgrade that reordered, widened or padded them would
// otherwise turn into a wrong draw count on a machine nobody here can run.
const _: () = {
    assert!(size_of::<ExecutionRange>() == 2 * size_of::<u32>());
    assert!(core::mem::offset_of!(ExecutionRange, location) == 0);
    assert!(core::mem::offset_of!(ExecutionRange, length) == size_of::<u32>());
};

/// Draws one [`draw_indirect_count`](crcbl_hal::CommandEncoder::draw_indirect_count)
/// may emit on this backend, and the `maxCommandCount` every ICB it builds is
/// bounded by.
///
/// **What makes it honest is that it is created, not asserted**: `limits_of`
/// reports it only after [`probe`] has watched this device return a live ICB of
/// exactly this many commands with exactly the descriptor the kernel encodes
/// for. A device that refuses gets neither the limit nor the feature.
///
/// It is far below the `1 << 20` `crcbl_mtl::adapter`'s ICB ladder watched CI's
/// Mac accept, and that is deliberate. The number is allocated for real, at
/// device-enumeration time and again on every call that asks for it, and an ICB
/// reserves storage for every command it could hold — so promising the ladder's
/// ceiling would mean a megabyte-scale allocation to answer a question and
/// another one per draw. This is the largest count the engine's own paths could
/// plausibly want (`crcbl_render::forward` asks for one per bucket) with a wide
/// margin, and raising it is a one-line change plus a rerun of the probe.
pub(crate) const MAX_DRAW_INDIRECT_COUNT: u32 = 4096;

/// The `[[kernel]]` that encodes a non-indexed draw per surviving structure.
pub(crate) const ENCODE_PLAIN: &str = "crcblEncodePlainDraws";
/// [`ENCODE_PLAIN`]'s indexed sibling for a `Uint16` index buffer.
pub(crate) const ENCODE_INDEXED_16: &str = "crcblEncodeIndexedDraws16";
/// [`ENCODE_PLAIN`]'s indexed sibling for a `Uint32` index buffer.
pub(crate) const ENCODE_INDEXED_32: &str = "crcblEncodeIndexedDraws32";

/// The kernel, in Metal Shading Language. See the module docs for why it is a
/// string here rather than a `.slang` source.
///
/// # Three entry points rather than one
///
/// MSL's `render_command::draw_indexed_primitives` takes the index buffer as a
/// **typed device pointer**, so `ushort` and `uint` indices are different
/// overloads — and a non-indexed encode has no index buffer to bind at all.
/// Metal requires every declared buffer argument to be bound, so one entry
/// point taking all three shapes would need a dummy binding for the two it is
/// not using. Three entry points in one library cost one compile and choose
/// themselves at the call site instead.
///
/// The execution range sits at `[[buffer(5)]]` in all three, leaving index four
/// vacant in the non-indexed one. A gap costs nothing — Metal's argument-table
/// indices need not be contiguous — and it buys one slot number per argument
/// across the whole library, so [`Encode::dispatch`] binds the same index every
/// time rather than one that depends on which entry point it chose.
///
/// # Thread zero writes the execution range
///
/// `CrcblExecutionRange` is `MTLIndirectCommandBufferExecutionRange` — see
/// [`ExecutionRange`], which asserts the two layouts agree — and the render
/// encoder's `executeCommandsInBuffer:indirectBuffer:indirectBufferOffset:`
/// reads the draw count out of it. One thread writes it because one value is
/// wanted, and thread zero exists in every dispatch this module makes: an ICB
/// of no commands is never allocated, because
/// [`plan_indirect_count`](crate::draw::plan_indirect_count) answers a
/// `max_draw_count` of zero with no plan at all.
///
/// # The argument structures are read as words
///
/// `a[0]`… rather than a struct, because the seam's buffer holds whatever the
/// producing pass wrote and the layouts are fixed by Metal's own headers:
/// `MTLDrawPrimitivesIndirectArguments` is `{ vertexCount, instanceCount,
/// vertexStart, baseInstance }` and `MTLDrawIndexedPrimitivesIndirectArguments`
/// is `{ indexCount, instanceCount, indexStart, baseVertex, baseInstance }`.
/// Reading words leaves no room for a struct-padding disagreement between the
/// two compilers.
///
/// # The primitive type is a switch and not a cast
///
/// `params.primitive` is an `MTLPrimitiveType` raw value, and the arms hand
/// `draw_primitives` a *literal* `primitive_type` rather than a value cast from
/// an integer. MSL's spec does not promise that a runtime-valued
/// `primitive_type` is accepted, and this code cannot be executed off a Mac —
/// so the construction that needs no promise is the one written. The arm for
/// `MTLPrimitiveType::Triangle` is the `default` rather than a `case`, which is
/// what makes every value `crcbl_mtl::conv`'s `primitive` cannot produce draw
/// triangles instead of points; [`primitive_code`] relies on that.
pub(crate) const ENCODE_MSL: &str = r"
#include <metal_stdlib>
using namespace metal;

struct CrcblEncodeTarget {
    command_buffer commands [[id(0)]];
};

struct CrcblEncodeParams {
    uint max_draws;
    uint args_stride;
    uint primitive;
};

struct CrcblExecutionRange {
    uint location;
    uint length;
};

#define CRCBL_PER_PRIMITIVE(DRAW)                        \
    switch (params.primitive) {                          \
    case 0: DRAW(primitive_type::point); break;          \
    case 1: DRAW(primitive_type::line); break;           \
    case 2: DRAW(primitive_type::line_strip); break;     \
    case 4: DRAW(primitive_type::triangle_strip); break; \
    default: DRAW(primitive_type::triangle); break;      \
    }

#define CRCBL_PLAIN_DRAW(PRIM) \
    command.draw_primitives(PRIM, a[2], a[0], a[1], a[3])

#define CRCBL_INDEXED_DRAW(PRIM) \
    command.draw_indexed_primitives(PRIM, a[0], indices + a[2], a[1], a[3], a[4])

#define CRCBL_DRAWS(params, counts) min((counts)[0], (params).max_draws)

#define CRCBL_PUBLISH_RANGE(index, params, counts, range) \
    if ((index) == 0) {                                   \
        (range).location = 0;                             \
        (range).length = CRCBL_DRAWS(params, counts);     \
    }

[[kernel]] void crcblEncodePlainDraws(
        device CrcblEncodeTarget& target [[buffer(0)]],
        const device uint* args [[buffer(1)]],
        const device uint* counts [[buffer(2)]],
        constant CrcblEncodeParams& params [[buffer(3)]],
        device CrcblExecutionRange& range [[buffer(5)]],
        uint index [[thread_position_in_grid]]) {
    if (index >= params.max_draws) { return; }
    CRCBL_PUBLISH_RANGE(index, params, counts, range)
    render_command command(target.commands, index);
    if (index >= CRCBL_DRAWS(params, counts)) { command.reset(); return; }
    const device uint* a = args + index * (params.args_stride / 4);
    CRCBL_PER_PRIMITIVE(CRCBL_PLAIN_DRAW)
}

[[kernel]] void crcblEncodeIndexedDraws16(
        device CrcblEncodeTarget& target [[buffer(0)]],
        const device uint* args [[buffer(1)]],
        const device uint* counts [[buffer(2)]],
        constant CrcblEncodeParams& params [[buffer(3)]],
        const device ushort* indices [[buffer(4)]],
        device CrcblExecutionRange& range [[buffer(5)]],
        uint index [[thread_position_in_grid]]) {
    if (index >= params.max_draws) { return; }
    CRCBL_PUBLISH_RANGE(index, params, counts, range)
    render_command command(target.commands, index);
    if (index >= CRCBL_DRAWS(params, counts)) { command.reset(); return; }
    const device uint* a = args + index * (params.args_stride / 4);
    CRCBL_PER_PRIMITIVE(CRCBL_INDEXED_DRAW)
}

[[kernel]] void crcblEncodeIndexedDraws32(
        device CrcblEncodeTarget& target [[buffer(0)]],
        const device uint* args [[buffer(1)]],
        const device uint* counts [[buffer(2)]],
        constant CrcblEncodeParams& params [[buffer(3)]],
        const device uint* indices [[buffer(4)]],
        device CrcblExecutionRange& range [[buffer(5)]],
        uint index [[thread_position_in_grid]]) {
    if (index >= params.max_draws) { return; }
    CRCBL_PUBLISH_RANGE(index, params, counts, range)
    render_command command(target.commands, index);
    if (index >= CRCBL_DRAWS(params, counts)) { command.reset(); return; }
    const device uint* a = args + index * (params.args_stride / 4);
    CRCBL_PER_PRIMITIVE(CRCBL_INDEXED_DRAW)
}
";

/// `CrcblEncodeParams`, field for field. Sent with `setBytes:length:atIndex:`,
/// so there is no buffer to keep alive and no offset to align.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Params {
    /// Commands in the ICB, and the number of threads dispatched. The kernel
    /// clamps the count buffer's word to it, which is the seam's
    /// `max_draw_count` doing its job.
    pub(crate) max_draws: u32,
    /// Bytes between argument structures. A multiple of four, which is what
    /// lets the kernel step a `uint` pointer by `args_stride / 4`.
    pub(crate) args_stride: u32,
    /// `MTLPrimitiveType`'s raw value; see [`ENCODE_MSL`].
    pub(crate) primitive: u32,
}

/// The compiled kernels, one per index shape.
///
/// **Built once per device, at `open`**, and only when the device reports
/// [`Features::DRAW_INDIRECT_COUNT`](crcbl_hal::Features::DRAW_INDIRECT_COUNT).
/// Eager rather than on first use because the flag is a promise: a compile
/// failure has to arrive as device creation failing, the way
/// `crcbl_mtl::pipeline`'s `default_depth_stencil_state` does, rather than as a
/// draw that refuses halfway through a frame.
#[derive(Debug)]
pub(crate) struct Kernels {
    plain: Kernel,
    indexed_16: Kernel,
    indexed_32: Kernel,
}

impl Kernels {
    /// Compiles [`ENCODE_MSL`] and specialises its three entry points.
    ///
    /// # Errors
    ///
    /// [`HalError::Backend`] when Metal's front end refuses the source, when an
    /// entry point is missing, or when a kernel cannot be specialised into a
    /// pipeline state. Each carries Metal's own `NSError` text, because that is
    /// the only diagnosis a caller gets.
    pub(crate) fn compile(device: &ProtocolObject<dyn MTLDevice>) -> Result<Self, HalError> {
        let library = device
            .newLibraryWithSource_options_error(&NSString::from_str(ENCODE_MSL), None)
            .map_err(|error| {
                HalError::Backend(format!(
                    "Metal's front end refused crcbl-mtl's indirect-count encoding kernel, which \
                     this backend reports DRAW_INDIRECT_COUNT on the strength of — {error}"
                ))
            })?;
        Ok(Self {
            plain: specialise(device, &library, ENCODE_PLAIN)?,
            indexed_16: specialise(device, &library, ENCODE_INDEXED_16)?,
            indexed_32: specialise(device, &library, ENCODE_INDEXED_32)?,
        })
    }

    /// The kernel for an encode of `index` shape — `None` for a non-indexed
    /// draw.
    fn for_indices(&self, format: Option<crcbl_hal::IndexFormat>) -> &Kernel {
        match format {
            None => &self.plain,
            Some(crcbl_hal::IndexFormat::Uint16) => &self.indexed_16,
            Some(crcbl_hal::IndexFormat::Uint32) => &self.indexed_32,
        }
    }
}

/// One entry point of [`ENCODE_MSL`] as a pipeline state.
fn specialise(
    device: &ProtocolObject<dyn MTLDevice>,
    library: &ProtocolObject<dyn MTLLibrary>,
    entry: &str,
) -> Result<Kernel, HalError> {
    let function = library
        .newFunctionWithName(&NSString::from_str(entry))
        .ok_or_else(|| {
            HalError::Backend(format!(
                "crcbl-mtl's indirect-count encoding kernel compiled but declares no [[kernel]] \
                 named {entry}"
            ))
        })?;
    device
        .newComputePipelineStateWithFunction_error(&function)
        .map_err(|error| {
            HalError::Backend(format!(
                "Metal compiled crcbl-mtl's {entry} and then refused to specialise it into an \
                 MTLComputePipelineState — {error}"
            ))
        })
}

/// The `MTLIndirectCommandBufferDescriptor` every ICB on this path is built
/// from.
///
/// **Inheriting both**, because the pass that called the seam verb has already
/// bound its pipeline state and its argument tables on the render encoder and
/// the kernel writes draw arguments alone. Both stage bind counts are zero for
/// the same reason: an inheriting command may not set buffers, so Metal's
/// defaults would size a per-command stride for binds nothing encodes.
fn descriptor(indexed: bool) -> Retained<MTLIndirectCommandBufferDescriptor> {
    let descriptor = MTLIndirectCommandBufferDescriptor::new();
    descriptor.setCommandTypes(if indexed {
        MTLIndirectCommandType::DrawIndexed
    } else {
        MTLIndirectCommandType::Draw
    });
    descriptor.setInheritPipelineState(true);
    descriptor.setInheritBuffers(true);
    descriptor.setMaxVertexBufferBindCount(0);
    descriptor.setMaxFragmentBufferBindCount(0);
    descriptor
}

/// An `MTLIndirectCommandBuffer` of `commands` inheriting draw commands, and
/// the argument buffer that hands its `gpuResourceID` to the kernel.
///
/// `Private` — [`MemoryLocation::DeviceLocal`] — because the CPU never touches
/// the ICB. The eight-byte handle table is `HostUpload`, whose seam contract is
/// exactly this: written once by the CPU, never read back.
///
/// # Errors
///
/// [`HalError::Backend`] when Metal returns nil for either allocation, which on
/// `newIndirectCommandBufferWithDescriptor:maxCommandCount:options:` is both
/// "the descriptor is illegal" and "the allocation failed".
fn allocate(
    device: &ProtocolObject<dyn MTLDevice>,
    indexed: bool,
    commands: u32,
) -> Result<(Icb, Buffer), HalError> {
    let count = to_ns(u64::from(commands));
    // SAFETY: `objc2` marks this unsafe because `maxCount` might not be
    // bounds-checked. It is at most `MAX_DRAW_INDIRECT_COUNT`, which
    // `crcbl_mtl::draw`'s `plan_indirect_count` refuses above and which this
    // device answered a live ICB for at enumeration; Metal's declared return is
    // `Option`, so a request it cannot satisfy comes back nil and is reported
    // below rather than unwound.
    let icb = unsafe {
        device.newIndirectCommandBufferWithDescriptor_maxCommandCount_options(
            &descriptor(indexed),
            count,
            conv::resource_options(MemoryLocation::DeviceLocal),
        )
    }
    .ok_or_else(|| {
        HalError::Backend(format!(
            "MTLDevice::newIndirectCommandBufferWithDescriptor:maxCommandCount:options: returned \
             nil for {commands} inheriting draw command(s)"
        ))
    })?;
    icb.setLabel(Some(&NSString::from_str("crcbl indirect-count commands")));

    let table = device
        .newBufferWithLength_options(
            to_ns(size_of::<MTLResourceID>() as u64),
            conv::resource_options(MemoryLocation::HostUpload),
        )
        .ok_or_else(|| {
            HalError::Backend(
                "MTLDevice::newBufferWithLength:options: returned nil for the eight-byte argument \
                 buffer an indirect-count encode hands its ICB through"
                    .to_string(),
            )
        })?;
    table.setLabel(Some(&NSString::from_str("crcbl indirect-count handle")));
    let handle = icb.gpuResourceID();
    // SAFETY: `contents` covers the bytes this buffer was just created with,
    // which is exactly one `MTLResourceID`; the allocation is `Shared`, so the
    // pointer is CPU-writable; and no GPU work has been submitted against it.
    unsafe {
        table
            .contents()
            .as_ptr()
            .cast::<MTLResourceID>()
            .write(handle);
    }
    Ok((icb, table))
}

/// The [`RANGE_BYTES`] the kernel writes this draw's execution range into and
/// the render encoder reads it back out of.
///
/// `Private` — [`MemoryLocation::DeviceLocal`] — for the reason the whole slice
/// exists: the count is the GPU's, and a location the CPU could read would
/// invite somebody to read it.
///
/// Separate from the handle table above rather than two fields of one
/// allocation, because the two have opposite directions. The table is
/// `HostUpload`, whose seam contract is "written once by the CPU, never read
/// back" — which is not a buffer a kernel should be writing into.
///
/// # Errors
///
/// [`HalError::Backend`] when Metal returns nil, which for an allocation this
/// size means the device is out of memory.
fn execution_range(device: &ProtocolObject<dyn MTLDevice>) -> Result<Buffer, HalError> {
    let range = device
        .newBufferWithLength_options(
            to_ns(RANGE_BYTES),
            conv::resource_options(MemoryLocation::DeviceLocal),
        )
        .ok_or_else(|| {
            HalError::Backend(format!(
                "MTLDevice::newBufferWithLength:options: returned nil for the {RANGE_BYTES}-byte \
                 execution range an indirect-count draw takes its command count from"
            ))
        })?;
    range.setLabel(Some(&NSString::from_str("crcbl indirect-count range")));
    Ok(range)
}

/// One queued kernel dispatch: everything `encode_render_pass` needs to write
/// one ICB before it opens the render encoder.
///
/// **Every field owns what it references**, which is the obligation deferral
/// creates — the same rule `crcbl_mtl::command`'s `RenderCommand` follows. A
/// buffer the caller destroys between the seam call and `end_render_pass` is
/// still there to be dispatched against.
pub(crate) struct Encode {
    /// The buffer the commands are written into, and the one the render
    /// encoder will execute.
    pub(crate) icb: Icb,
    /// Where the kernel publishes how many of [`Self::icb`]'s commands to run.
    /// Handed to `executeCommandsInBuffer:indirectBuffer:indirectBufferOffset:`
    /// at offset zero.
    pub(crate) range: Buffer,
    /// The argument buffer holding [`Self::icb`]'s `gpuResourceID`.
    table: Buffer,
    kernel: Kernel,
    args: Buffer,
    args_offset: NSUInteger,
    counts: Buffer,
    count_offset: NSUInteger,
    /// The bound index buffer and the byte offset of index zero, for an indexed
    /// encode. `None` makes this a non-indexed one, and the kernel it names
    /// declares no index argument.
    indices: Option<(Buffer, NSUInteger)>,
    params: Params,
}

impl core::fmt::Debug for Encode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Encode")
            .field("params", &self.params)
            .field("indexed", &self.indices.is_some())
            .finish_non_exhaustive()
    }
}

/// Everything a `draw_indirect_count` resolved before its ICB was allocated.
///
/// A plain parameter object rather than eight arguments, because
/// [`Encode::plan`] is called from one place and the order of four
/// `Retained<MTLBuffer>`s is not something a reader should have to check.
pub(crate) struct Request<'a> {
    pub(crate) device: &'a ProtocolObject<dyn MTLDevice>,
    pub(crate) kernels: &'a Kernels,
    pub(crate) args: Buffer,
    pub(crate) counts: Buffer,
    /// The bound index buffer, for an indexed draw. `None` selects the
    /// non-indexed kernel, which declares no index argument at all.
    pub(crate) indices: Option<Indices>,
    pub(crate) primitive: MTLPrimitiveType,
    /// The plan `crcbl_mtl::draw`'s `plan_indirect_count` produced, which is
    /// where both buffer offsets come from.
    pub(crate) plan: crate::draw::IndirectCountPlan,
}

/// The index buffer an indexed encode writes into each command.
pub(crate) struct Indices {
    pub(crate) raw: Buffer,
    /// Which of [`ENCODE_MSL`]'s two indexed kernels reads it — MSL takes the
    /// index buffer as a typed device pointer, so the width is the entry point
    /// rather than an argument.
    pub(crate) format: crcbl_hal::IndexFormat,
    /// Byte offset of index zero.
    pub(crate) offset: u64,
}

impl Encode {
    /// Allocates the ICB a call needs and queues the dispatch that will fill
    /// it.
    ///
    /// # Errors
    ///
    /// Whatever [`allocate`] reports — a nil ICB or a nil argument buffer — or
    /// what [`execution_range`] does.
    pub(crate) fn plan(request: Request<'_>) -> Result<Self, HalError> {
        let format = request.indices.as_ref().map(|indices| indices.format);
        let (icb, table) = allocate(request.device, format.is_some(), request.plan.max_draws)?;
        Ok(Self {
            icb,
            range: execution_range(request.device)?,
            table,
            kernel: request.kernels.for_indices(format).clone(),
            args: request.args,
            args_offset: to_ns(request.plan.args_offset),
            counts: request.counts,
            count_offset: to_ns(request.plan.count_offset),
            indices: request
                .indices
                .map(|indices| (indices.raw, to_ns(indices.offset))),
            params: Params {
                max_draws: request.plan.max_draws,
                args_stride: request.plan.stride,
                primitive: primitive_code(request.primitive),
            },
        })
    }

    /// Dispatches the kernel on an already-open compute encoder.
    ///
    /// Sizing is the shape `crcbl_mtl::command`'s `dispatch` uses — whole
    /// threadgroups, because `dispatchThreads:` needs a non-uniform-threadgroup
    /// family this backend does not query for. The kernel's own
    /// `index >= max_draws` guard is what makes the rounding up harmless, and
    /// it is the *only* thing that does.
    /// Hands the encoded commands to the driver on an already-open blit
    /// encoder, between the kernel that wrote them and the pass that runs them.
    ///
    /// **This is the one structural difference between the probes that pass and
    /// the frame that hung.** `crcbl_mtl::device`'s two ICB rungs both `commit`
    /// and `waitUntilCompleted` between encoding an ICB and executing it, so
    /// kernel-write and execute land in *separate* command buffers. A frame does
    /// both in one, and Apple's own GPU-encoding sample calls this in exactly
    /// that position. Nothing here can execute Metal, so this is reasoning from
    /// the difference plus Apple's guidance rather than a measurement — the
    /// `mtl e2e` job is what settles it.
    ///
    /// The whole command count, because the kernel writes every index: the ones
    /// past the count are `reset()` rather than left undefined, which is what
    /// makes optimising the full range meaningful.
    pub(crate) fn optimize(&self, encoder: &ProtocolObject<dyn MTLBlitCommandEncoder>) {
        let range = NSRange {
            location: 0,
            length: to_ns(u64::from(self.params.max_draws)),
        };
        // SAFETY: `encoder` is open and belongs to the command buffer this
        // encode's dispatch was recorded on, and `self.icb` is the buffer that
        // dispatch writes. The range is the count the ICB was created with.
        unsafe {
            encoder.optimizeIndirectCommandBuffer_withRange(&self.icb, range);
        }
    }

    pub(crate) fn dispatch(&self, encoder: &ProtocolObject<dyn MTLComputeCommandEncoder>) {
        encoder.setComputePipelineState(&self.kernel);
        // SAFETY for every setter below: `objc2` marks them unsafe because
        // Metal bounds-checks neither the offset nor the argument-table index,
        // and because the contents must be of the type the shader declared.
        // Each index is the `[[buffer(n)]]` `ENCODE_MSL` declares for that
        // argument; each offset was bounded against the buffer's own length by
        // `crcbl_mtl::draw`'s `plan_indirect_count` before this was built; and
        // every buffer is a `Retained` this value owns.
        unsafe {
            encoder.setBuffer_offset_atIndex(Some(&self.table), 0, 0);
            encoder.setBuffer_offset_atIndex(Some(&self.args), self.args_offset, 1);
            encoder.setBuffer_offset_atIndex(Some(&self.counts), self.count_offset, 2);
            encoder.setBytes_length_atIndex(
                core::ptr::NonNull::from(&self.params).cast::<core::ffi::c_void>(),
                to_ns(size_of::<Params>() as u64),
                3,
            );
            if let Some((raw, offset)) = &self.indices {
                encoder.setBuffer_offset_atIndex(Some(raw), *offset, 4);
            }
            encoder.setBuffer_offset_atIndex(Some(&self.range), 0, 5);
        }
        // Not optional: the kernel reaches the ICB through a handle Metal
        // cannot follow back to a resource, so this is both the residency
        // declaration and the write hazard the render encoder's execute is
        // ordered against.
        //
        // The execution range needs no counterpart. It is bound above and named
        // again in the execute, so it is an ordinary tracked buffer at both
        // ends of the hazard and Metal follows it without being told.
        encoder.useResource_usage(
            ProtocolObject::from_ref(&*self.icb),
            MTLResourceUsage::Write,
        );
        let per_group = self
            .kernel
            .maxTotalThreadsPerThreadgroup()
            .min(to_ns(u64::from(self.params.max_draws)))
            .max(1);
        let groups = u64::from(self.params.max_draws).div_ceil(per_group as u64);
        encoder.dispatchThreadgroups_threadsPerThreadgroup(
            MTLSize {
                width: to_ns(groups),
                height: 1,
                depth: 1,
            },
            MTLSize {
                width: per_group,
                height: 1,
                depth: 1,
            },
        );
    }
}

/// `MTLPrimitiveType`'s raw value, which is what [`ENCODE_MSL`]'s switch reads.
///
/// Forwarded rather than re-tabulated: `objc2-metal` declares the type as a
/// transparent newtype over Metal's own enumerator, so a table here would be a
/// second copy of a mapping the MSL switch already spells out.
///
/// `NSUInteger` is the wider type and MSL's argument is 32 bits, so the
/// conversion has a failure case no `MTLPrimitiveType` can reach. It lands on a
/// value no arm of that switch names, which is its `default` — a triangle, and
/// the only choice that cannot silently turn a triangle list into points.
fn primitive_code(primitive: MTLPrimitiveType) -> u32 {
    u32::try_from(primitive.0).unwrap_or(u32::MAX)
}

/// Whether this device can serve the indirect-count path, and with how many
/// commands.
///
/// **This is what makes both the feature and the limit measurements rather than
/// claims.** It builds the exact descriptor [`descriptor`] builds and asks for
/// exactly [`MAX_DRAW_INDIRECT_COUNT`] commands of it — indexed, which is the
/// larger per-command stride of the two and so the harder allocation — and
/// answers `false` if anything comes back nil.
///
/// What it deliberately does **not** do is compile [`ENCODE_MSL`]. Enumeration
/// walks every `MTLDevice` in the machine and a shader compile is the expensive
/// half; the kernels are built once, on the device that is actually opened, and
/// `MetalDevice::open` fails if they will not build. So the split is: this
/// answers "can this device hold the commands", and device creation answers
/// "will this device compile the kernel that writes them".
pub(crate) fn probe(device: &ProtocolObject<dyn MTLDevice>) -> bool {
    allocate(device, true, MAX_DRAW_INDIRECT_COUNT).is_ok()
}
