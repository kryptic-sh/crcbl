//! Device Removed Extended Data — the operation a removed device died on.
//!
//! # The failure this module exists for
//!
//! [`crate::debug`]'s `diagnosis` turns `DXGI_ERROR_DEVICE_REMOVED` into a
//! *reason*, and a reason is one `HRESULT`. `0x887A0005` says the adapter went
//! away; it does not say which draw, dispatch, copy or barrier was in flight
//! when it did, and the call that reports the removal is never the call that
//! caused it. On a machine nobody can attach a debugger to — every CI runner
//! this project has — that is the whole of the evidence.
//!
//! DRED is Microsoft's answer to exactly that. With auto-breadcrumbs on, the
//! runtime writes a marker per GPU operation into memory the driver keeps across
//! a device removal, so after the fact the removed device can be asked which
//! operations each command list finished and which one it was still inside. That
//! last one is the failing work.
//!
//! # Two halves, and the order between them is the whole thing
//!
//! * [`enable`] runs **before the first `D3D12CreateDevice`**, because DRED is
//!   chosen per device at creation exactly as the debug layer is.
//!   [`crate::Dx12Instance`]'s `open` discharges that ordering for both.
//!   Enabling it afterwards produces a device with no breadcrumbs and **no
//!   error** — which is the failure mode this module would otherwise be a
//!   thorough implementation of.
//! * [`diagnosis`] runs **after the removal**, on the same `ID3D12Device`.
//!   `ID3D12DeviceRemovedExtendedData` is a query on the dead device, so the
//!   object has to still be alive; every caller reaches this through
//!   `crate::debug::diagnosis`, which is handed the device it is reporting on.
//!
//! # Why it is always on, and not behind `CRCBL_DX12_VALIDATION`
//!
//! That variable means "this machine has and wants the debug layer", and the
//! debug layer is an optional Windows component — the *Graphics Tools* feature
//! on demand. DRED is not: Microsoft documents it as part of the D3D12 runtime
//! from Windows 10 1809 onward, needing nothing installed, and its cost is a
//! breadcrumb write per operation rather than the whole validation layer, which
//! is why shipping titles leave it on. Two further reasons decide it:
//!
//! * **A device removal is not reproducible on demand.** A diagnostic that has
//!   to be asked for in advance is one that is off the first time it is needed,
//!   and the first time is usually the only time anybody has the failing run.
//! * **`CRCBL_DX12_VALIDATION=0` says nothing about DRED.** It is the escape
//!   hatch for a machine with no Graphics Tools feature; letting it also switch
//!   breadcrumbs off would take the diagnostics away from precisely the bare
//!   runners that have no other way to see anything.
//!
//! This crate cannot check any of the above: it is developed on Linux and no
//! machine here runs D3D12. What is asserted below is the arithmetic and the
//! formatting; `docs/backlog.md` records what is not.
//!
//! # Walking driver memory
//!
//! Everything DRED returns is raw pointers into memory the driver owns: a linked
//! list of nodes, an array of operations per node, a linked list of allocations.
//! A diagnostic that runs off the end of one of those turns a removed device
//! into a second crash, which is strictly worse than the opaque `HRESULT` it
//! replaced. So every walk here is bounded by a predicate the test build checks —
//! [`Breadcrumbs::wants_more`], [`Allocations::wants_more`] and [`op_window`] —
//! every pointer is null-checked at the step that dereferences it, and no count
//! reported by the driver is used without being clamped against the extent it
//! describes.

#[cfg(target_os = "windows")]
use std::sync::OnceLock;

#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Direct3D12::{
    D3D12_AUTO_BREADCRUMB_NODE, D3D12_AUTO_BREADCRUMB_OP, D3D12_DRED_ALLOCATION_NODE,
    D3D12_DRED_ENABLEMENT_FORCED_ON, D3D12GetDebugInterface, ID3D12Device,
    ID3D12DeviceRemovedExtendedData, ID3D12DeviceRemovedExtendedDataSettings,
};
#[cfg(target_os = "windows")]
use windows::core::Interface;

/// How many breadcrumb nodes a report quotes.
///
/// The nodes are command lists, and a removal that took several of them down is
/// answered by the first few: past that the lines stop being read. The count of
/// nodes actually seen is kept exact in [`Breadcrumbs::seen`], which is the
/// shape `crate::debug`'s `ValidationReport` uses for the same reason.
const MAX_NODES: usize = 8;

/// How many breadcrumb nodes the walk will visit at all.
///
/// A linked list in driver memory after a device removal is not a structure to
/// trust: a corrupt or cyclic `pNext` would otherwise be an infinite loop inside
/// the report of a crash. This is the bound that makes that impossible, and
/// [`Breadcrumbs::wants_more`] is where it is applied.
const MAX_NODES_WALKED: usize = 64;

/// How many allocations of each page-fault list a report quotes.
const MAX_ALLOCATIONS: usize = 8;

/// How many allocations of each page-fault list the walk will visit.
/// [`MAX_NODES_WALKED`]'s argument, for the other linked list.
const MAX_ALLOCATIONS_WALKED: usize = 64;

/// How many completed operations before the boundary a report quotes.
///
/// The interesting one is the operation the GPU was inside; the ones before it
/// are the context that says what kind of work the list was doing.
const OPS_BEFORE: u32 = 8;

/// How many operations from the boundary onward a report quotes, the boundary
/// itself included. What was *not* reached says which way the list was headed.
const OPS_AFTER: u32 = 4;

/// The longest debug name copied out of driver memory, in characters.
///
/// A `SetName` string is a label a program chose, so this is generous rather
/// than tight — but it is a bound, because the pointer is the driver's and a
/// missing terminator would otherwise be a scan through unmapped memory.
#[cfg(target_os = "windows")]
const MAX_NAME_CHARS: usize = 128;

/// The name a line uses for an object DRED did not name.
///
/// A word rather than an empty pair of quotes, because "this object had no
/// `SetName`" and "this object is named the empty string" are different facts
/// and the second one is not what happened.
const UNNAMED: &str = "<unnamed>";

/// Every `D3D12_AUTO_BREADCRUMB_OP`, indexed by its own value.
///
/// Spelled as names in a dense table here for the reason `crate::debug`'s
/// `ALLOWED` spells message ids as numbers: what a report *says* has to be
/// decidable — and testable — on a machine with no D3D12 at all. The prefix is
/// dropped from each name because every line already says it is a breadcrumb.
///
/// The `const` assertion below is what keeps a position and the constant that
/// belongs there equal, and it is checked by the `x86_64-pc-windows-msvc` build.
const BREADCRUMB_OPS: &[&str] = &[
    "SETMARKER",
    "BEGINEVENT",
    "ENDEVENT",
    "DRAWINSTANCED",
    "DRAWINDEXEDINSTANCED",
    "EXECUTEINDIRECT",
    "DISPATCH",
    "COPYBUFFERREGION",
    "COPYTEXTUREREGION",
    "COPYRESOURCE",
    "COPYTILES",
    "RESOLVESUBRESOURCE",
    "CLEARRENDERTARGETVIEW",
    "CLEARUNORDEREDACCESSVIEW",
    "CLEARDEPTHSTENCILVIEW",
    "RESOURCEBARRIER",
    "EXECUTEBUNDLE",
    "PRESENT",
    "RESOLVEQUERYDATA",
    "BEGINSUBMISSION",
    "ENDSUBMISSION",
    "DECODEFRAME",
    "PROCESSFRAMES",
    "ATOMICCOPYBUFFERUINT",
    "ATOMICCOPYBUFFERUINT64",
    "RESOLVESUBRESOURCEREGION",
    "WRITEBUFFERIMMEDIATE",
    "DECODEFRAME1",
    "SETPROTECTEDRESOURCESESSION",
    "DECODEFRAME2",
    "PROCESSFRAMES1",
    "BUILDRAYTRACINGACCELERATIONSTRUCTURE",
    "EMITRAYTRACINGACCELERATIONSTRUCTUREPOSTBUILDINFO",
    "COPYRAYTRACINGACCELERATIONSTRUCTURE",
    "DISPATCHRAYS",
    "INITIALIZEMETACOMMAND",
    "EXECUTEMETACOMMAND",
    "ESTIMATEMOTION",
    "RESOLVEMOTIONVECTORHEAP",
    "SETPIPELINESTATE1",
    "INITIALIZEEXTENSIONCOMMAND",
    "EXECUTEEXTENSIONCOMMAND",
    "DISPATCHMESH",
    "ENCODEFRAME",
    "RESOLVEENCODEROUTPUTMETADATA",
    "BARRIER",
    "BEGIN_COMMAND_LIST",
    "DISPATCHGRAPH",
    "SETPROGRAM",
];

/// The D3D12 constants [`BREADCRUMB_OPS`] names, in the same order.
///
/// Its only purpose is the assertion below: a table of names written against
/// positions is one nothing checks, and this is the check.
#[cfg(target_os = "windows")]
const BREADCRUMB_OP_CONSTANTS: &[D3D12_AUTO_BREADCRUMB_OP] = &[
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_SETMARKER,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_BEGINEVENT,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_ENDEVENT,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_DRAWINSTANCED,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_DRAWINDEXEDINSTANCED,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_EXECUTEINDIRECT,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_DISPATCH,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_COPYBUFFERREGION,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_COPYTEXTUREREGION,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_COPYRESOURCE,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_COPYTILES,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_RESOLVESUBRESOURCE,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_CLEARRENDERTARGETVIEW,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_CLEARUNORDEREDACCESSVIEW,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_CLEARDEPTHSTENCILVIEW,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_RESOURCEBARRIER,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_EXECUTEBUNDLE,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_PRESENT,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_RESOLVEQUERYDATA,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_BEGINSUBMISSION,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_ENDSUBMISSION,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_DECODEFRAME,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_PROCESSFRAMES,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_ATOMICCOPYBUFFERUINT,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_ATOMICCOPYBUFFERUINT64,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_RESOLVESUBRESOURCEREGION,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_WRITEBUFFERIMMEDIATE,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_DECODEFRAME1,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_SETPROTECTEDRESOURCESESSION,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_DECODEFRAME2,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_PROCESSFRAMES1,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_BUILDRAYTRACINGACCELERATIONSTRUCTURE,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_EMITRAYTRACINGACCELERATIONSTRUCTUREPOSTBUILDINFO,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_COPYRAYTRACINGACCELERATIONSTRUCTURE,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_DISPATCHRAYS,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_INITIALIZEMETACOMMAND,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_EXECUTEMETACOMMAND,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_ESTIMATEMOTION,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_RESOLVEMOTIONVECTORHEAP,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_SETPIPELINESTATE1,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_INITIALIZEEXTENSIONCOMMAND,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_EXECUTEEXTENSIONCOMMAND,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_DISPATCHMESH,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_ENCODEFRAME,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_RESOLVEENCODEROUTPUTMETADATA,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_BARRIER,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_BEGIN_COMMAND_LIST,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_DISPATCHGRAPH,
    windows::Win32::Graphics::Direct3D12::D3D12_AUTO_BREADCRUMB_OP_SETPROGRAM,
];

// **The position is the constant, or this does not compile.** `BREADCRUMB_OPS`
// is indexed by the raw op value, so an entry sitting at the wrong index would
// name every operation after it wrongly — silently, in the one report anybody
// reads. This is the check, and it runs wherever the crate is built for Windows.
#[cfg(target_os = "windows")]
const _: () = {
    assert!(
        BREADCRUMB_OPS.len() == BREADCRUMB_OP_CONSTANTS.len(),
        "the breadcrumb op names and the D3D12 constants they claim to be are different lengths"
    );
    let mut index = 0;
    while index < BREADCRUMB_OP_CONSTANTS.len() {
        assert!(
            BREADCRUMB_OP_CONSTANTS[index].0 == index as i32,
            "a D3D12_AUTO_BREADCRUMB_OP constant does not sit at its own value in BREADCRUMB_OPS"
        );
        index += 1;
    }
};

/// Every `D3D12_DRED_ALLOCATION_TYPE`, as its raw value and the name D3D12 gives
/// it.
///
/// A list rather than [`BREADCRUMB_OPS`]' dense table because these values are
/// not dense: they continue an older object-type enumeration and start at 19.
const ALLOCATION_TYPES: &[(i32, &str)] = &[
    (-1, "INVALID"),
    (19, "COMMAND_QUEUE"),
    (20, "COMMAND_ALLOCATOR"),
    (21, "PIPELINE_STATE"),
    (22, "COMMAND_LIST"),
    (23, "FENCE"),
    (24, "DESCRIPTOR_HEAP"),
    (25, "HEAP"),
    (27, "QUERY_HEAP"),
    (28, "COMMAND_SIGNATURE"),
    (29, "PIPELINE_LIBRARY"),
    (30, "VIDEO_DECODER"),
    (32, "VIDEO_PROCESSOR"),
    (34, "RESOURCE"),
    (35, "PASS"),
    (36, "CRYPTOSESSION"),
    (37, "CRYPTOSESSIONPOLICY"),
    (38, "PROTECTEDRESOURCESESSION"),
    (39, "VIDEO_DECODER_HEAP"),
    (40, "COMMAND_POOL"),
    (41, "COMMAND_RECORDER"),
    (42, "STATE_OBJECT"),
    (43, "METACOMMAND"),
    (44, "SCHEDULINGGROUP"),
    (45, "VIDEO_MOTION_ESTIMATOR"),
    (46, "VIDEO_MOTION_VECTOR_HEAP"),
    (47, "VIDEO_EXTENSION_COMMAND"),
    (48, "VIDEO_ENCODER"),
    (49, "VIDEO_ENCODER_HEAP"),
];

/// The D3D12 constants [`ALLOCATION_TYPES`] names, in the same order. See
/// [`BREADCRUMB_OP_CONSTANTS`] for why it exists.
#[cfg(target_os = "windows")]
const ALLOCATION_TYPE_CONSTANTS:
    &[windows::Win32::Graphics::Direct3D12::D3D12_DRED_ALLOCATION_TYPE] = &[
    windows::Win32::Graphics::Direct3D12::D3D12_DRED_ALLOCATION_TYPE_INVALID,
    windows::Win32::Graphics::Direct3D12::D3D12_DRED_ALLOCATION_TYPE_COMMAND_QUEUE,
    windows::Win32::Graphics::Direct3D12::D3D12_DRED_ALLOCATION_TYPE_COMMAND_ALLOCATOR,
    windows::Win32::Graphics::Direct3D12::D3D12_DRED_ALLOCATION_TYPE_PIPELINE_STATE,
    windows::Win32::Graphics::Direct3D12::D3D12_DRED_ALLOCATION_TYPE_COMMAND_LIST,
    windows::Win32::Graphics::Direct3D12::D3D12_DRED_ALLOCATION_TYPE_FENCE,
    windows::Win32::Graphics::Direct3D12::D3D12_DRED_ALLOCATION_TYPE_DESCRIPTOR_HEAP,
    windows::Win32::Graphics::Direct3D12::D3D12_DRED_ALLOCATION_TYPE_HEAP,
    windows::Win32::Graphics::Direct3D12::D3D12_DRED_ALLOCATION_TYPE_QUERY_HEAP,
    windows::Win32::Graphics::Direct3D12::D3D12_DRED_ALLOCATION_TYPE_COMMAND_SIGNATURE,
    windows::Win32::Graphics::Direct3D12::D3D12_DRED_ALLOCATION_TYPE_PIPELINE_LIBRARY,
    windows::Win32::Graphics::Direct3D12::D3D12_DRED_ALLOCATION_TYPE_VIDEO_DECODER,
    windows::Win32::Graphics::Direct3D12::D3D12_DRED_ALLOCATION_TYPE_VIDEO_PROCESSOR,
    windows::Win32::Graphics::Direct3D12::D3D12_DRED_ALLOCATION_TYPE_RESOURCE,
    windows::Win32::Graphics::Direct3D12::D3D12_DRED_ALLOCATION_TYPE_PASS,
    windows::Win32::Graphics::Direct3D12::D3D12_DRED_ALLOCATION_TYPE_CRYPTOSESSION,
    windows::Win32::Graphics::Direct3D12::D3D12_DRED_ALLOCATION_TYPE_CRYPTOSESSIONPOLICY,
    windows::Win32::Graphics::Direct3D12::D3D12_DRED_ALLOCATION_TYPE_PROTECTEDRESOURCESESSION,
    windows::Win32::Graphics::Direct3D12::D3D12_DRED_ALLOCATION_TYPE_VIDEO_DECODER_HEAP,
    windows::Win32::Graphics::Direct3D12::D3D12_DRED_ALLOCATION_TYPE_COMMAND_POOL,
    windows::Win32::Graphics::Direct3D12::D3D12_DRED_ALLOCATION_TYPE_COMMAND_RECORDER,
    windows::Win32::Graphics::Direct3D12::D3D12_DRED_ALLOCATION_TYPE_STATE_OBJECT,
    windows::Win32::Graphics::Direct3D12::D3D12_DRED_ALLOCATION_TYPE_METACOMMAND,
    windows::Win32::Graphics::Direct3D12::D3D12_DRED_ALLOCATION_TYPE_SCHEDULINGGROUP,
    windows::Win32::Graphics::Direct3D12::D3D12_DRED_ALLOCATION_TYPE_VIDEO_MOTION_ESTIMATOR,
    windows::Win32::Graphics::Direct3D12::D3D12_DRED_ALLOCATION_TYPE_VIDEO_MOTION_VECTOR_HEAP,
    windows::Win32::Graphics::Direct3D12::D3D12_DRED_ALLOCATION_TYPE_VIDEO_EXTENSION_COMMAND,
    windows::Win32::Graphics::Direct3D12::D3D12_DRED_ALLOCATION_TYPE_VIDEO_ENCODER,
    windows::Win32::Graphics::Direct3D12::D3D12_DRED_ALLOCATION_TYPE_VIDEO_ENCODER_HEAP,
];

// **The number is the constant, or this does not compile.** See
// `BREADCRUMB_OP_CONSTANTS` above; this is the same check for the sparse table.
#[cfg(target_os = "windows")]
const _: () = {
    assert!(
        ALLOCATION_TYPES.len() == ALLOCATION_TYPE_CONSTANTS.len(),
        "the allocation type names and the D3D12 constants they claim to be are different lengths"
    );
    let mut index = 0;
    while index < ALLOCATION_TYPES.len() {
        assert!(
            ALLOCATION_TYPES[index].0 == ALLOCATION_TYPE_CONSTANTS[index].0,
            "a D3D12_DRED_ALLOCATION_TYPE in ALLOCATION_TYPES is not the constant it names"
        );
        index += 1;
    }
};

/// The name D3D12 gives a breadcrumb operation, or `None` for a value this build
/// has no name for.
///
/// Reported by number rather than folded into a neighbour when it is unknown: a
/// newer runtime's operation is not something to mislabel as an older one.
fn op_name(raw: i32) -> Option<&'static str> {
    usize::try_from(raw)
        .ok()
        .and_then(|index| BREADCRUMB_OPS.get(index))
        .copied()
}

/// The name D3D12 gives an allocation type, or `None` for a value this build has
/// no name for.
fn allocation_type_name(raw: i32) -> Option<&'static str> {
    ALLOCATION_TYPES
        .iter()
        .find(|(value, _)| *value == raw)
        .map(|(_, name)| *name)
}

/// Which slice of a command list's history is worth quoting, and the bound the
/// walk reads it under.
///
/// `completed` is what `pLastBreadcrumbValue` holds — the number of operations
/// the GPU finished — and `recorded` is `BreadcrumbCount`, the extent of the
/// history array. **Neither is trusted.** `completed` is clamped to `recorded`
/// before anything is derived from it, because a driver that reported more
/// completions than operations would otherwise index past the array, and
/// `recorded` bounds the end. The result is therefore always inside the array
/// the driver described, and never longer than [`OPS_BEFORE`] + [`OPS_AFTER`].
fn op_window(completed: u32, recorded: u32) -> core::ops::Range<u32> {
    let boundary = completed.min(recorded);
    let start = boundary.saturating_sub(OPS_BEFORE);
    let end = boundary.saturating_add(OPS_AFTER).min(recorded);
    start..end
}

/// One command list DRED kept breadcrumbs for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct BreadcrumbNode {
    /// The command list's debug name, when it had one.
    pub(crate) command_list: Option<String>,
    /// The command queue's debug name, when it had one. `crate::device`'s
    /// `open` puts the caller's `DeviceDesc::label` here.
    pub(crate) command_queue: Option<String>,
    /// How many of the recorded operations the GPU finished.
    pub(crate) completed: u32,
    /// How many operations the list recorded.
    pub(crate) recorded: u32,
    /// The index in the history that `ops[0]` came from, so a line can name an
    /// operation by its real position rather than by its position in the quote.
    pub(crate) first_op: u32,
    /// The window [`op_window`] chose, as raw `D3D12_AUTO_BREADCRUMB_OP` values.
    pub(crate) ops: Vec<i32>,
}

impl BreadcrumbNode {
    /// This node's lines, the header first and one per quoted operation.
    fn lines(&self, index: usize) -> Vec<String> {
        let mut lines = vec![format!(
            "  node {index}: command list {:?} on queue {:?} — {} of {} op(s) completed",
            self.command_list.as_deref().unwrap_or(UNNAMED),
            self.command_queue.as_deref().unwrap_or(UNNAMED),
            self.completed,
            self.recorded,
        )];
        if self.first_op > 0 {
            lines.push(format!("    … {} earlier op(s) not shown", self.first_op));
        }
        for (offset, raw) in self.ops.iter().enumerate() {
            let position = self.first_op.saturating_add(
                u32::try_from(offset).unwrap_or_else(|_| unreachable!("the window fits in a u32")),
            );
            let name = op_name(*raw).map_or_else(
                || format!("D3D12_AUTO_BREADCRUMB_OP {raw}, which this build has no name for"),
                ToString::to_string,
            );
            lines.push(format!(
                "    op #{position} {name} ({})",
                self.state_of(position)
            ));
        }
        let quoted = self.first_op.saturating_add(
            u32::try_from(self.ops.len())
                .unwrap_or_else(|_| unreachable!("the window fits in a u32")),
        );
        if quoted < self.recorded {
            lines.push(format!(
                "    … {} later op(s) not shown",
                self.recorded - quoted
            ));
        }
        lines
    }

    /// What the GPU had done with the operation at `position`.
    ///
    /// The boundary is the whole point of the report: `completed` operations
    /// finished, so the one *at* index `completed` is the one that was still
    /// running when the device went away. A list whose every operation completed
    /// has no such index, and says so rather than pointing past its own end.
    fn state_of(&self, position: u32) -> &'static str {
        let boundary = self.completed.min(self.recorded);
        if position < boundary {
            "completed"
        } else if position == boundary && boundary < self.recorded {
            "IN FLIGHT — the GPU had not finished this one"
        } else {
            "not reached"
        }
    }
}

/// The breadcrumb nodes a report kept, and how many there were.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Breadcrumbs {
    /// Up to [`MAX_NODES`] nodes, in the order DRED listed them.
    pub(crate) kept: Vec<BreadcrumbNode>,
    /// How many nodes the walk visited, including those past [`MAX_NODES`].
    pub(crate) seen: usize,
}

impl Breadcrumbs {
    /// Records one node, keeping it if there is room and counting it either way.
    fn record(&mut self, node: BreadcrumbNode) {
        self.seen += 1;
        if self.kept.len() < MAX_NODES {
            self.kept.push(node);
        }
    }

    /// Whether the walk may take another `pNext`.
    ///
    /// **This is the bound, not a suggestion.** The list lives in driver memory
    /// belonging to a device that has just died, so a `pNext` that loops or
    /// points at rubbish is a case the walk has to survive rather than one it
    /// may assume away.
    fn wants_more(&self) -> bool {
        self.seen < MAX_NODES_WALKED
    }

    /// Whether the walk stopped because it ran out of budget rather than because
    /// the list ended. Reported, because a truncated list and a complete one are
    /// different claims about a failure.
    fn walk_was_cut(&self) -> bool {
        !self.wants_more()
    }

    /// The header and every kept node's lines.
    fn lines(&self) -> Vec<String> {
        let mut lines = vec![format!(
            "DRED auto-breadcrumbs: {} command list(s) with recorded work{}{}",
            self.seen,
            if self.seen > self.kept.len() {
                format!(", showing the first {}", self.kept.len())
            } else {
                String::new()
            },
            if self.walk_was_cut() {
                format!(
                    "; the walk stopped at its {MAX_NODES_WALKED}-node limit, so the list may be longer"
                )
            } else {
                String::new()
            },
        )];
        for (index, node) in self.kept.iter().enumerate() {
            lines.extend(node.lines(index));
        }
        lines
    }
}

/// One allocation DRED named beside a page fault.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Allocation {
    /// The object's debug name, when it had one.
    pub(crate) name: Option<String>,
    /// Its raw `D3D12_DRED_ALLOCATION_TYPE`.
    pub(crate) kind: i32,
}

impl Allocation {
    /// The one-line form.
    fn line(&self, what: &str) -> String {
        let kind = allocation_type_name(self.kind).map_or_else(
            || {
                format!(
                    "D3D12_DRED_ALLOCATION_TYPE {}, which this build has no name for",
                    self.kind
                )
            },
            ToString::to_string,
        );
        format!(
            "    {what} allocation {:?} ({kind})",
            self.name.as_deref().unwrap_or(UNNAMED)
        )
    }
}

/// One of a page fault's two allocation lists.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Allocations {
    /// Up to [`MAX_ALLOCATIONS`] allocations, in the order DRED listed them.
    pub(crate) kept: Vec<Allocation>,
    /// How many the walk visited, including those past [`MAX_ALLOCATIONS`].
    pub(crate) seen: usize,
}

impl Allocations {
    /// Records one allocation, keeping it if there is room.
    fn record(&mut self, allocation: Allocation) {
        self.seen += 1;
        if self.kept.len() < MAX_ALLOCATIONS {
            self.kept.push(allocation);
        }
    }

    /// Whether the walk may take another `pNext`. [`Breadcrumbs::wants_more`]'s
    /// argument, for the other linked list.
    fn wants_more(&self) -> bool {
        self.seen < MAX_ALLOCATIONS_WALKED
    }

    /// One line per kept allocation, plus a count of the rest.
    fn lines(&self, what: &str) -> Vec<String> {
        let mut lines: Vec<String> = self
            .kept
            .iter()
            .map(|allocation| allocation.line(what))
            .collect();
        if self.seen > self.kept.len() {
            lines.push(format!(
                "    … {} more {what} allocation(s) not shown",
                self.seen - self.kept.len()
            ));
        }
        lines
    }
}

/// The virtual address a removed device faulted on, and what was mapped there.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct PageFault {
    /// The faulting GPU virtual address.
    pub(crate) address: u64,
    /// Allocations still live that cover or neighbour the address.
    pub(crate) existing: Allocations,
    /// Allocations recently freed that used to. This is the list that catches a
    /// use-after-free of a resource the CPU released too early.
    pub(crate) freed: Allocations,
}

impl PageFault {
    /// Whether this says nothing at all.
    ///
    /// DRED answers the page-fault query on a device that was not removed by a
    /// page fault, and what it writes then is a zeroed address and two empty
    /// lists. Printing that would claim a fault at address zero, which is worse
    /// than printing nothing.
    fn is_empty(&self) -> bool {
        self.address == 0 && self.existing.seen == 0 && self.freed.seen == 0
    }

    /// The header and both allocation lists.
    fn lines(&self) -> Vec<String> {
        let mut lines = vec![format!(
            "DRED page fault at GPU virtual address {:#018X}",
            self.address
        )];
        lines.extend(self.existing.lines("existing"));
        lines.extend(self.freed.lines("recently freed"));
        lines
    }
}

/// Everything DRED had to say about a removed device.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Report {
    /// The breadcrumbs, or `None` when the runtime kept none — which is what it
    /// answers for a device that never submitted work, and for one created
    /// before [`enable`] ran.
    pub(crate) breadcrumbs: Option<Breadcrumbs>,
    /// The page fault, or `None` when the removal was not one.
    pub(crate) page_fault: Option<PageFault>,
}

impl Report {
    /// Every line this report contributes to a device-removed message.
    ///
    /// Never empty: a report with nothing in it says so, because "DRED printed
    /// nothing" and "DRED was not consulted" are the two claims this whole
    /// module exists to keep apart.
    pub(crate) fn lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if let Some(breadcrumbs) = &self.breadcrumbs {
            lines.extend(breadcrumbs.lines());
        }
        if let Some(page_fault) = &self.page_fault
            && !page_fault.is_empty()
        {
            lines.extend(page_fault.lines());
        }
        if lines.is_empty() {
            lines.push(
                "DRED: the runtime kept no breadcrumbs and reported no page fault for this device \
                 — the removal happened outside recorded GPU work, or this device was created \
                 before DRED was turned on"
                    .to_string(),
            );
        }
        lines
    }
}

/// Whether DRED was asked for, and whether the runtime took it.
///
/// A [`OnceLock`] for the reason `crate::debug`'s `DEBUG_LAYER` is one: the
/// settings are process-wide and have to be in place before the first
/// `D3D12CreateDevice`, so a second instance opening later must not set them
/// again and must not log a second time about it either.
#[cfg(target_os = "windows")]
static DRED: OnceLock<bool> = OnceLock::new();

/// Turns DRED auto-breadcrumbs and page-fault reporting on for this process.
///
/// **Call this before the first `D3D12CreateDevice`.** DRED is chosen per device
/// at creation exactly as the debug layer is, so a device opened before this ran
/// keeps no breadcrumbs — and reports no error saying so, which is why the
/// ordering is discharged in one place, [`crate::Dx12Instance`]'s `open`.
///
/// Returns whether the settings were applied. A runtime too old to offer them —
/// `ID3D12DeviceRemovedExtendedDataSettings` arrived in Windows 10 1809 — is a
/// **warning, not a failure**: it costs the diagnosis, not the engine.
#[cfg(target_os = "windows")]
pub(crate) fn enable() -> bool {
    *DRED.get_or_init(|| {
        let mut settings: Option<ID3D12DeviceRemovedExtendedDataSettings> = None;
        // SAFETY: `settings` is a live out-parameter the call writes an interface
        // through, and `ID3D12DeviceRemovedExtendedDataSettings` is the IID it is
        // asked for — so the QI either succeeds or the call fails, and a failure
        // leaves it `None`, which is why it is read back rather than assumed.
        if let Err(error) = unsafe { D3D12GetDebugInterface(&mut settings) } {
            crcbl_core::log::warn!(
                "crcbl-dx12: this runtime has no ID3D12DeviceRemovedExtendedDataSettings \
                 ({error}), so a device removal will name an HRESULT and no operation"
            );
            return false;
        }
        let Some(settings) = settings else {
            crcbl_core::log::warn!(
                "crcbl-dx12: D3D12GetDebugInterface reported success and wrote no DRED settings"
            );
            return false;
        };
        // SAFETY: `settings` is the interface the call just returned. Both
        // methods take a scalar enumerant and return nothing; the enablement is
        // process-wide and outlives this interface, which is what makes it sound
        // to drop it here.
        unsafe {
            settings.SetAutoBreadcrumbsEnablement(D3D12_DRED_ENABLEMENT_FORCED_ON);
            settings.SetPageFaultEnablement(D3D12_DRED_ENABLEMENT_FORCED_ON);
        }
        crcbl_core::log::info!("crcbl-dx12: DRED auto-breadcrumbs and page-fault reporting are on");
        true
    })
}

/// Whether [`enable`] applied the settings. `false` before it has run.
#[cfg(target_os = "windows")]
fn dred_on() -> bool {
    DRED.get().copied().unwrap_or(false)
}

/// Copies a null-terminated UTF-16 debug name out of driver memory.
///
/// Bounded at [`MAX_NAME_CHARS`] and marked with an ellipsis when it stops
/// there, because a missing terminator in a driver's own memory would otherwise
/// be a scan off the end of a mapping.
///
/// # Safety
///
/// `ptr` is null, or points at up to [`MAX_NAME_CHARS`] readable `u16`s.
#[cfg(target_os = "windows")]
unsafe fn wide_name(ptr: *const u16) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let mut units: Vec<u16> = Vec::new();
    let mut terminated = false;
    for index in 0..MAX_NAME_CHARS {
        // SAFETY: the caller guarantees `MAX_NAME_CHARS` readable `u16`s from
        // `ptr`, and `index` is below that bound.
        let unit = unsafe { ptr.add(index).read() };
        if unit == 0 {
            terminated = true;
            break;
        }
        units.push(unit);
    }
    if units.is_empty() {
        return None;
    }
    let mut name = String::from_utf16_lossy(&units);
    if !terminated {
        name.push('…');
    }
    Some(name)
}

/// Copies a null-terminated narrow debug name out of driver memory. The
/// counterpart of [`wide_name`], and bounded the same way.
///
/// # Safety
///
/// `ptr` is null, or points at up to [`MAX_NAME_CHARS`] readable bytes.
#[cfg(target_os = "windows")]
unsafe fn narrow_name(ptr: *const u8) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let mut bytes: Vec<u8> = Vec::new();
    let mut terminated = false;
    for index in 0..MAX_NAME_CHARS {
        // SAFETY: the caller guarantees `MAX_NAME_CHARS` readable bytes from
        // `ptr`, and `index` is below that bound.
        let byte = unsafe { ptr.add(index).read() };
        if byte == 0 {
            terminated = true;
            break;
        }
        bytes.push(byte);
    }
    if bytes.is_empty() {
        return None;
    }
    let mut name = String::from_utf8_lossy(&bytes).into_owned();
    if !terminated {
        name.push('…');
    }
    Some(name)
}

/// The wide name if there is one, otherwise the narrow one.
///
/// D3D12 fills whichever `SetName` was given, so a node carries at most one.
///
/// # Safety
///
/// Both pointers meet [`wide_name`]'s and [`narrow_name`]'s contracts.
#[cfg(target_os = "windows")]
unsafe fn object_name(wide: windows::core::PCWSTR, narrow: *const u8) -> Option<String> {
    // SAFETY: the caller's guarantee, forwarded unchanged.
    unsafe { wide_name(wide.as_ptr()).or_else(|| narrow_name(narrow)) }
}

/// Reads one command list's breadcrumbs.
///
/// # Safety
///
/// `node` points at a live `D3D12_AUTO_BREADCRUMB_NODE` the runtime wrote, whose
/// `pCommandHistory` covers `BreadcrumbCount` operations and whose name pointers
/// meet [`object_name`]'s contract.
#[cfg(target_os = "windows")]
unsafe fn read_node(node: &D3D12_AUTO_BREADCRUMB_NODE) -> BreadcrumbNode {
    // SAFETY: the caller's guarantee about the node's four name pointers.
    let command_list =
        unsafe { object_name(node.pCommandListDebugNameW, node.pCommandListDebugNameA) };
    // SAFETY: as above.
    let command_queue =
        unsafe { object_name(node.pCommandQueueDebugNameW, node.pCommandQueueDebugNameA) };
    let completed = if node.pLastBreadcrumbValue.is_null() {
        0
    } else {
        // SAFETY: the pointer is non-null and the runtime keeps a `u32` there
        // for exactly this read.
        unsafe { node.pLastBreadcrumbValue.read() }
    };
    let window = op_window(completed, node.BreadcrumbCount);
    let first_op = window.start;
    let mut ops = Vec::new();
    if !node.pCommandHistory.is_null() {
        for index in window {
            let Ok(index) = usize::try_from(index) else {
                break;
            };
            // SAFETY: `op_window` clamped `index` below `BreadcrumbCount`, which
            // is the extent the caller guarantees `pCommandHistory` covers, and
            // `D3D12_AUTO_BREADCRUMB_OP` is a `Copy` newtype over an `i32`.
            let op: D3D12_AUTO_BREADCRUMB_OP = unsafe { node.pCommandHistory.add(index).read() };
            ops.push(op.0);
        }
    }
    BreadcrumbNode {
        command_list,
        command_queue,
        completed,
        recorded: node.BreadcrumbCount,
        first_op,
        ops,
    }
}

/// Walks the auto-breadcrumb list.
///
/// # Safety
///
/// `head` is null, or points at a `D3D12_AUTO_BREADCRUMB_NODE` list the runtime
/// wrote. A `pNext` that loops or is invalid is *not* assumed away — the walk is
/// bounded by [`Breadcrumbs::wants_more`] and null-checks every step — but each
/// non-null `pNext` the runtime wrote must be readable as a node.
#[cfg(target_os = "windows")]
unsafe fn walk_breadcrumbs(head: *const D3D12_AUTO_BREADCRUMB_NODE) -> Breadcrumbs {
    let mut breadcrumbs = Breadcrumbs::default();
    let mut cursor = head;
    while !cursor.is_null() && breadcrumbs.wants_more() {
        // SAFETY: `cursor` is non-null and the caller guarantees it is a node
        // the runtime wrote. The borrow ends before `cursor` advances, and
        // nothing here clones or drops the interface fields the node carries.
        let node = unsafe { &*cursor };
        // SAFETY: as above, and the node's own pointers are the runtime's.
        breadcrumbs.record(unsafe { read_node(node) });
        cursor = node.pNext;
    }
    breadcrumbs
}

/// Walks one of a page fault's allocation lists.
///
/// # Safety
///
/// [`walk_breadcrumbs`]' contract, for `D3D12_DRED_ALLOCATION_NODE`.
#[cfg(target_os = "windows")]
unsafe fn walk_allocations(head: *const D3D12_DRED_ALLOCATION_NODE) -> Allocations {
    let mut allocations = Allocations::default();
    let mut cursor = head;
    while !cursor.is_null() && allocations.wants_more() {
        // SAFETY: `cursor` is non-null and the caller guarantees it is a node the
        // runtime wrote.
        let node = unsafe { &*cursor };
        // SAFETY: the node's name pointers are the runtime's and meet
        // `object_name`'s contract.
        let name = unsafe { object_name(node.ObjectNameW, node.ObjectNameA) };
        allocations.record(Allocation {
            name,
            kind: node.AllocationType.0,
        });
        cursor = node.pNext;
    }
    allocations
}

/// Everything DRED can say about `device`, ready to append to a device-removed
/// message.
///
/// **Only worth calling on a device that has actually been removed.** Both
/// queries answer `DXGI_ERROR_NOT_CURRENTLY_AVAILABLE` on a healthy one, which
/// this reads as "nothing to say" rather than as an error — so
/// `crate::debug::diagnosis` calls it inside the arm where
/// `GetDeviceRemovedReason` has already answered.
///
/// Returns the lines directly rather than a [`Report`], because the two ways
/// there is nothing to report — the runtime has no `ID3D12DeviceRemovedExtendedData`
/// at all, and it has one that kept nothing — are different sentences and only
/// the second is a [`Report`].
#[cfg(target_os = "windows")]
pub(crate) fn diagnosis(device: &ID3D12Device) -> Vec<String> {
    let data = match device.cast::<ID3D12DeviceRemovedExtendedData>() {
        Ok(data) => data,
        Err(error) => {
            return vec![format!(
                "DRED: this device has no ID3D12DeviceRemovedExtendedData ({error}), so nothing \
                 here names the operation — DRED needs Windows 10 1809 or newer"
            )];
        }
    };
    let mut lines = Vec::new();
    if !dred_on() {
        lines.push(
            "DRED: crcbl_dx12::dred::enable did not run before this device was created, so any \
             breadcrumbs below are whatever the system default kept"
                .to_string(),
        );
    }
    // SAFETY: `data` is the interface the QI above returned. The call takes no
    // pointer of ours and writes the output struct it returns by value.
    let breadcrumbs = unsafe { data.GetAutoBreadcrumbsOutput() }
        .ok()
        // SAFETY: the head pointer is the runtime's, and `walk_breadcrumbs` is
        // bounded and null-checks every step of the list behind it.
        .map(|output| unsafe { walk_breadcrumbs(output.pHeadAutoBreadcrumbNode) });
    // SAFETY: as above.
    let page_fault = unsafe { data.GetPageFaultAllocationOutput() }
        .ok()
        .map(|output| PageFault {
            address: output.PageFaultVA,
            // SAFETY: both heads are the runtime's, and `walk_allocations` is
            // bounded and null-checks every step.
            existing: unsafe { walk_allocations(output.pHeadExistingAllocationNode) },
            // SAFETY: as above.
            freed: unsafe { walk_allocations(output.pHeadRecentFreedAllocationNode) },
        });
    lines.extend(
        Report {
            breadcrumbs,
            page_fault,
        }
        .lines(),
    );
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The op table is indexed by the op's own value, so a shifted entry
    /// renames every operation after it.**
    ///
    /// Red if an entry is inserted or removed without the value it belongs to
    /// moving with it. The `const` assertion above ties these positions to the
    /// D3D12 constants, but only on Windows; this half runs everywhere and is
    /// what catches a table edited on the Linux box this backend is written on.
    #[test]
    fn the_breadcrumb_op_table_is_dense_and_named_by_position() {
        assert_eq!(op_name(0), Some("SETMARKER"));
        assert_eq!(op_name(3), Some("DRAWINSTANCED"));
        assert_eq!(op_name(5), Some("EXECUTEINDIRECT"));
        assert_eq!(op_name(15), Some("RESOURCEBARRIER"));
        assert_eq!(op_name(42), Some("DISPATCHMESH"));
        assert_eq!(op_name(48), Some("SETPROGRAM"));

        // Past the end and below it are reported as unknown rather than folded
        // into a neighbour: a newer runtime's op is not an older one.
        assert_eq!(op_name(-1), None);
        assert_eq!(
            op_name(i32::try_from(BREADCRUMB_OPS.len()).expect("the table fits in an i32")),
            None
        );

        let mut names: Vec<&str> = BREADCRUMB_OPS.to_vec();
        let listed = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), listed, "two breadcrumb ops share a name");
    }

    /// **Every allocation type is listed once, and an unknown one stays
    /// unknown.**
    #[test]
    fn the_allocation_type_table_is_a_lookup_and_not_a_guess() {
        assert_eq!(allocation_type_name(-1), Some("INVALID"));
        assert_eq!(allocation_type_name(22), Some("COMMAND_LIST"));
        assert_eq!(allocation_type_name(34), Some("RESOURCE"));
        // 26, 31 and 33 are gaps in D3D12's own numbering.
        assert_eq!(allocation_type_name(26), None);
        assert_eq!(allocation_type_name(1000), None);

        let mut values: Vec<i32> = ALLOCATION_TYPES.iter().map(|(value, _)| *value).collect();
        let listed = values.len();
        values.sort_unstable();
        values.dedup();
        assert_eq!(values.len(), listed, "an allocation type is listed twice");
    }

    /// **The window never leaves the array the driver described, whatever the
    /// driver said.**
    ///
    /// This is the arithmetic that bounds an `unsafe` read into driver memory,
    /// so the cases that matter are the dishonest ones: more completions than
    /// operations, a zero-length history, and a boundary at either end.
    #[test]
    fn the_op_window_stays_inside_the_history_the_driver_described() {
        // The ordinary case: context before the boundary, and a look past it.
        let window = op_window(41, 57);
        assert_eq!(window, 33..45);

        // Near the start there is nothing before the boundary to quote.
        assert_eq!(op_window(2, 57), 0..6);
        assert_eq!(op_window(0, 57), 0..4);

        // A list that finished entirely stops at its own end.
        assert_eq!(op_window(57, 57), 49..57);

        // **A driver that reports more completions than operations must not
        // index past the array.** Nothing else in this module re-checks it.
        assert_eq!(op_window(u32::MAX, 57), 49..57);
        assert_eq!(op_window(9000, 4), 0..4);

        // An empty history is empty, not a one-element read at index zero.
        assert_eq!(op_window(0, 0), 0..0);
        assert_eq!(op_window(u32::MAX, 0), 0..0);

        // And in general: inside the extent, and no longer than the two caps.
        for recorded in [0u32, 1, 7, 64, u32::MAX] {
            for completed in [0u32, 1, 7, 64, recorded, u32::MAX] {
                let window = op_window(completed, recorded);
                assert!(window.start <= window.end, "{completed}/{recorded}");
                assert!(window.end <= recorded, "{completed}/{recorded}");
                assert!(
                    u64::from(window.end) - u64::from(window.start)
                        <= u64::from(OPS_BEFORE) + u64::from(OPS_AFTER),
                    "{completed}/{recorded}"
                );
            }
        }
    }

    fn node(completed: u32, recorded: u32, ops: &[i32]) -> BreadcrumbNode {
        let window = op_window(completed, recorded);
        BreadcrumbNode {
            command_list: Some("shadow pass".to_string()),
            command_queue: Some("crcbl main queue".to_string()),
            completed,
            recorded,
            first_op: window.start,
            ops: ops.to_vec(),
        }
    }

    /// **The line that names the failing work.**
    ///
    /// The whole module is for this one sentence: the operation at the boundary
    /// is the one the GPU had not finished, and it has to be distinguishable
    /// from the ones before and after it. Red if the boundary moves by one, if
    /// a completed op starts reading as in flight, or if the position stops
    /// being the op's real index in the history.
    #[test]
    fn the_boundary_op_is_the_one_marked_in_flight() {
        // 41 of 57 completed, so the window is 33..45 and #41 is the failure.
        let lines = node(41, 57, &[3, 15, 4, 5, 6, 15, 3, 15, 42, 20, 3, 15]).lines(0);
        let text = lines.join("\n");

        assert!(
            text.contains("command list \"shadow pass\" on queue \"crcbl main queue\" — 41 of 57"),
            "{text}"
        );
        assert!(text.contains("… 33 earlier op(s) not shown"), "{text}");
        assert!(
            text.contains("op #40 RESOURCEBARRIER (completed)"),
            "{text}"
        );
        assert!(
            text.contains("op #41 DISPATCHMESH (IN FLIGHT — the GPU had not finished this one)"),
            "{text}"
        );
        assert!(
            text.contains("op #42 ENDSUBMISSION (not reached)"),
            "{text}"
        );
        assert!(text.contains("… 12 later op(s) not shown"), "{text}");

        // Exactly one op is ever in flight, or the report names two failures.
        assert_eq!(
            text.matches("IN FLIGHT").count(),
            1,
            "one op is in flight, not {text}"
        );
    }

    /// **A list that finished every op has no op in flight, and says so by
    /// having none rather than by pointing past its own end.**
    #[test]
    fn a_command_list_that_finished_names_no_failing_op() {
        let text = node(4, 4, &[3, 15, 4, 5]).lines(0).join("\n");
        assert!(!text.contains("IN FLIGHT"), "{text}");
        assert!(text.contains("op #3 EXECUTEINDIRECT (completed)"), "{text}");
        assert!(!text.contains("later op(s) not shown"), "{text}");
    }

    /// **An op this build has no name for is reported by number, not guessed
    /// at.**
    #[test]
    fn an_unknown_op_is_reported_by_number() {
        let text = node(0, 1, &[9001]).lines(0).join("\n");
        assert!(
            text.contains("op #0 D3D12_AUTO_BREADCRUMB_OP 9001, which this build has no name for"),
            "{text}"
        );
    }

    /// **An object DRED did not name still gets a line.**
    #[test]
    fn an_unnamed_command_list_still_reports_its_counts() {
        let text = BreadcrumbNode {
            completed: 1,
            recorded: 2,
            ops: vec![3, 4],
            ..BreadcrumbNode::default()
        }
        .lines(2)
        .join("\n");
        assert!(
            text.contains("node 2: command list \"<unnamed>\" on queue \"<unnamed>\" — 1 of 2"),
            "{text}"
        );
    }

    /// **The walk is bounded, and the counts past the bound stay exact.**
    ///
    /// `wants_more` is the predicate the `unsafe` list walk runs under, so what
    /// this asserts is that a cyclic `pNext` in driver memory terminates: the
    /// walk stops at [`MAX_NODES_WALKED`] and says it stopped there, rather than
    /// running until the process dies a second time.
    #[test]
    fn the_breadcrumb_walk_stops_at_its_limit_and_says_so() {
        let mut breadcrumbs = Breadcrumbs::default();
        let mut visited = 0usize;
        while breadcrumbs.wants_more() {
            breadcrumbs.record(node(0, 0, &[]));
            visited += 1;
            assert!(visited <= MAX_NODES_WALKED, "the walk did not terminate");
        }
        assert_eq!(visited, MAX_NODES_WALKED);
        assert_eq!(breadcrumbs.seen, MAX_NODES_WALKED);
        assert_eq!(breadcrumbs.kept.len(), MAX_NODES, "the quote is capped");
        assert!(breadcrumbs.walk_was_cut());

        let header = &breadcrumbs.lines()[0];
        assert!(
            header.contains(&format!("{MAX_NODES_WALKED} command list(s)")),
            "{header}"
        );
        assert!(
            header.contains(&format!("showing the first {MAX_NODES}")),
            "{header}"
        );
        assert!(header.contains("stopped at its"), "{header}");

        // A short list is neither capped nor cut, and does not claim to be.
        let mut short = Breadcrumbs::default();
        short.record(node(1, 2, &[3, 4]));
        assert!(!short.walk_was_cut());
        let header = &short.lines()[0];
        assert!(
            header.contains("1 command list(s) with recorded work"),
            "{header}"
        );
        assert!(!header.contains("showing the first"), "{header}");
        assert!(!header.contains("stopped at its"), "{header}");
    }

    /// **The allocation walk is bounded the same way, and its counts stay
    /// exact past the quote.**
    #[test]
    fn the_allocation_walk_stops_at_its_limit_and_counts_the_rest() {
        let mut allocations = Allocations::default();
        let mut visited = 0usize;
        while allocations.wants_more() {
            allocations.record(Allocation {
                name: Some("staging".to_string()),
                kind: 34,
            });
            visited += 1;
            assert!(
                visited <= MAX_ALLOCATIONS_WALKED,
                "the walk did not terminate"
            );
        }
        assert_eq!(visited, MAX_ALLOCATIONS_WALKED);
        assert_eq!(allocations.kept.len(), MAX_ALLOCATIONS);

        let lines = allocations.lines("existing");
        assert_eq!(lines.len(), MAX_ALLOCATIONS + 1);
        assert!(
            lines[0].contains("existing allocation \"staging\" (RESOURCE)"),
            "{}",
            lines[0]
        );
        assert!(
            lines[MAX_ALLOCATIONS].contains(&format!(
                "{} more existing allocation(s) not shown",
                MAX_ALLOCATIONS_WALKED - MAX_ALLOCATIONS
            )),
            "{}",
            lines[MAX_ALLOCATIONS]
        );
    }

    /// **A page fault names the address and both lists, and a device that did
    /// not fault claims no fault at address zero.**
    #[test]
    fn a_page_fault_names_the_address_and_says_nothing_when_there_was_none() {
        let mut existing = Allocations::default();
        existing.record(Allocation {
            name: Some("gbuffer".to_string()),
            kind: 34,
        });
        let mut freed = Allocations::default();
        freed.record(Allocation {
            name: None,
            kind: 22,
        });
        let fault = PageFault {
            address: 0x0000_1234_5678_9ABC,
            existing,
            freed,
        };
        assert!(!fault.is_empty());
        let text = fault.lines().join("\n");
        assert!(
            text.contains("DRED page fault at GPU virtual address 0x0000123456789ABC"),
            "{text}"
        );
        assert!(
            text.contains("existing allocation \"gbuffer\" (RESOURCE)"),
            "{text}"
        );
        assert!(
            text.contains("recently freed allocation \"<unnamed>\" (COMMAND_LIST)"),
            "{text}"
        );

        // The shape DRED writes for a removal that was not a page fault. It
        // falls through to the empty report's line, which says there was no
        // fault — what must never appear is the header claiming one at zero.
        assert!(PageFault::default().is_empty());
        let empty = Report {
            breadcrumbs: None,
            page_fault: Some(PageFault::default()),
        }
        .lines()
        .join("\n");
        assert!(
            !empty.contains("DRED page fault at"),
            "a zeroed page-fault output must not be reported as a fault at address zero: {empty}"
        );
        assert!(empty.contains("reported no page fault"), "{empty}");
    }

    /// **A report with nothing in it says so.**
    ///
    /// "DRED printed nothing" and "DRED was never consulted" read identically in
    /// a log unless one of them writes a line, and the whole point of this
    /// module is that a removal stops being silent.
    #[test]
    fn an_empty_report_still_says_something() {
        let lines = Report::default().lines();
        assert_eq!(lines.len(), 1);
        assert!(
            lines[0].contains("kept no breadcrumbs and reported no page fault"),
            "{}",
            lines[0]
        );
    }
}
