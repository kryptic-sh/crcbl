//! GPU queries — timestamps first.
//!
//! Timestamp queries are in the seam from P0 on purpose. The engine's debug
//! principle is that **profiling hooks live in the seam itself**
//! (`docs/notes/backends.md`'s HAL rules): per-pass GPU timers feed the render
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
//! # One rule lives here, and the rest are obligations on a backend
//!
//! The types are `Copy`, `PartialEq` and a field layout the compiler checks,
//! and everything this module *documents* about them is an obligation on a
//! backend — both halves of the degrading rule above are checked where they are
//! implemented, in `crate::null`'s
//! `timestamp_queries_read_back_zeros_without_failing` and
//! `the_portable_preset_refuses_query_kinds_it_lacks`.
//!
//! [`QueryKind::check_supported`] is the exception, and it is here rather than
//! in five backends because the thing it refuses is missing from *this* module:
//! see [`QueryKind::Occlusion`].

use crcbl_core::Handle;

/// Marker type for query-set handles. Uninhabited.
#[derive(Debug)]
pub enum QuerySet {}

/// A pool of queries of one kind.
pub type QuerySetHandle = Handle<QuerySet>;

/// What a query set measures.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum QueryKind {
    /// A GPU clock reading, taken at a pass boundary named by
    /// [`PassTimestampWrites`](crate::PassTimestampWrites).
    ///
    /// [`Device::query_results`](crate::Device::query_results) reports it in
    /// **nanoseconds** — the backend converts from whatever its API counts in,
    /// because the conversion is the one part of a timestamp that has no
    /// cross-API spelling. [`resolve_query_set`](crate::CommandEncoder::resolve_query_set)
    /// writes the device's own values instead, unconverted; it is a GPU-side
    /// copy with nothing to multiply by.
    Timestamp,
    /// Samples that passed the depth test between begin and end.
    ///
    /// **No backend will make you one**, and
    /// [`check_supported`](Self::check_supported) is where each of them says so:
    /// there is no begin and no end to count between, because
    /// [`CommandEncoder`](crate::CommandEncoder) has no verb that opens a query
    /// around a draw. The variant stays so that the day one arrives it is a
    /// backend arm rather than a seam change; until then the kind is refused
    /// rather than served, for the reason
    /// [`NO_OCCLUSION_QUERY_VERB`](crate::NO_OCCLUSION_QUERY_VERB) gives.
    ///
    /// The engine's own occlusion culling never wanted this: it is a two-phase
    /// depth pyramid in compute (topic 03 §3.3), so nothing here is waiting on
    /// the verb.
    Occlusion,
    /// Primitive and invocation counts.
    PipelineStatistics,
}

impl QueryKind {
    /// Refuses [`Occlusion`](Self::Occlusion), the kind no backend serves.
    ///
    /// Every backend's
    /// [`create_query_set`](crate::Device::create_query_set) runs this before it
    /// builds anything, so the one refusal every one of them makes is written
    /// once and reads the same wherever a caller meets it — the shape
    /// [`ShaderStages::check_supported`](crate::ShaderStages::check_supported)
    /// and [`ImageViewDesc::check`](crate::ImageViewDesc::check) already use for
    /// a rule the whole seam keeps.
    ///
    /// **It is the seam that refuses, not the device**, which is why there is no
    /// [`Features`](crate::Features) argument: Vulkan, D3D12, Metal and WebGPU
    /// can all count samples, and
    /// [`Features::OCCLUSION_QUERY`](crate::Features::OCCLUSION_QUERY) goes on
    /// reporting that. What is absent is the verb that would scope a count, and
    /// no device supplies one. [`NO_OCCLUSION_QUERY_VERB`](crate::NO_OCCLUSION_QUERY_VERB)
    /// carries the whole argument, and is the same sentence
    /// [`Device::supports`](crate::Device::supports) declares and
    /// [`DIVERGENCES`](crate::DIVERGENCES) records.
    ///
    /// # Errors
    ///
    /// [`HalError::Unsupported`](crate::HalError::Unsupported) for
    /// [`Occlusion`](Self::Occlusion), attributed to `backend` so a caller reads
    /// the refusal in the voice of the backend it asked.
    pub fn check_supported(self, backend: crate::BackendKind) -> Result<(), crate::HalError> {
        match self {
            Self::Occlusion => Err(crate::HalError::Unsupported {
                backend,
                what: crate::NO_OCCLUSION_QUERY_VERB,
            }),
            Self::Timestamp | Self::PipelineStatistics => Ok(()),
        }
    }
}

/// Creation parameters for a query set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuerySetDesc<'a> {
    /// Debug name; see [`BufferDesc::label`](crate::BufferDesc::label).
    pub label: Option<&'a str>,
    /// What it measures.
    pub kind: QueryKind,
    /// How many queries it holds. Query indices are `0..count`.
    ///
    /// Never zero: see
    /// [`Device::create_query_set`](crate::Device::create_query_set), which
    /// refuses one on every backend.
    pub count: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BackendKind, HalError};

    /// **The one kind the seam refuses, and the two it does not.**
    ///
    /// Both halves are the check: an `Err` for [`QueryKind::Occlusion`] alone
    /// says nothing while the other two could be refused too, and this is the
    /// rule five `create_query_set` implementations delegate to, so a version
    /// that refused everything would take the timestamp path down with it.
    ///
    /// **What turns it red.** The occlusion arm answering `Ok` — which is the
    /// change that puts a pool nothing can write back in a caller's hands — or
    /// either other arm answering `Err`.
    #[test]
    fn only_the_occlusion_kind_is_refused_and_it_is_refused_for_the_asking_backend() {
        for backend in [
            BackendKind::Vulkan,
            BackendKind::WebGpu,
            BackendKind::Metal,
            BackendKind::Dx12,
            BackendKind::Null,
        ] {
            let refused = QueryKind::Occlusion.check_supported(backend);
            assert!(
                matches!(
                    refused,
                    Err(HalError::Unsupported { backend: named, what })
                        if named == backend && what == crate::NO_OCCLUSION_QUERY_VERB
                ),
                "{backend}: {refused:?}"
            );
            QueryKind::Timestamp
                .check_supported(backend)
                .expect("a timestamp set is the kind this seam does fill");
            QueryKind::PipelineStatistics
                .check_supported(backend)
                .expect("the statistics kind is refused per backend, not here");
        }
    }
}
