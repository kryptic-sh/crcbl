//! GPU queries — timestamps first.
//!
//! Timestamp queries are in the seam from P0 on purpose. The engine's debug
//! principle is that **profiling hooks live in the seam itself**
//! (`docs/plan/01-foundations.md` §1.3): per-pass GPU timers feed the render
//! graph's frame-timing report at P1 and the profiler HUD at P10, and a
//! profiler bolted on afterwards is one that never covers the passes written
//! before it.
//!
//! # Degrading, not breaking
//!
//! [`Features::TIMESTAMP_QUERY`](crate::Features::TIMESTAMP_QUERY) is optional
//! because WebGPU's timestamp support is browser-dependent (topic 10's risk
//! list). A backend without it must accept
//! [`PassTimestampWrites`](crate::PassTimestampWrites) on a pass descriptor as a
//! no-op and return zeros from
//! [`Device::query_results`](crate::Device::query_results) — the HUD shows
//! blanks, the frame still renders.
//!
//! # There are no unit tests in this module
//!
//! It declares three types and no behaviour: `Copy`, `PartialEq` and the field
//! layout are derives the compiler checks, and everything else this module
//! documents is an obligation on a *backend*. Both halves of the degrading rule
//! above are checked where they are implemented — `crate::null`'s
//! `timestamp_queries_read_back_zeros_without_failing` and
//! `the_portable_preset_refuses_query_kinds_it_lacks`. A test here could only rebuild a
//! [`QuerySetDesc`] and read its own literals back, which is what used to be
//! here.

use crcbl_core::Handle;

/// Marker type for query-set handles. Uninhabited.
#[derive(Debug)]
pub enum QuerySet {}

/// A pool of queries of one kind.
pub type QuerySetHandle = Handle<QuerySet>;

/// What a query set measures.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum QueryKind {
    /// A GPU clock tick, written at a pass boundary named by
    /// [`PassTimestampWrites`](crate::PassTimestampWrites). Convert
    /// deltas to nanoseconds with
    /// [`Limits::timestamp_period_ns`](crate::Limits::timestamp_period_ns).
    Timestamp,
    /// Samples that passed the depth test between begin and end.
    ///
    /// Reserved: the engine's occlusion culling is a two-phase depth pyramid in
    /// compute (topic 03 §3.3), not hardware occlusion queries, and it is
    /// post-MVP either way.
    Occlusion,
    /// Primitive and invocation counts.
    PipelineStatistics,
}

/// Creation parameters for a query set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuerySetDesc<'a> {
    /// Debug name; see [`BufferDesc::label`](crate::BufferDesc::label).
    pub label: Option<&'a str>,
    /// What it measures.
    pub kind: QueryKind,
    /// How many queries it holds. Query indices are `0..count`.
    pub count: u32,
}
