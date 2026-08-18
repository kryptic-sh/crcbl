//! What a query result occupies, where one lands in a destination buffer, and
//! what a timestamp tick is worth.
//!
//! # The stride is D3D12's, and the seam assumes it is eight
//!
//! `ID3D12GraphicsCommandList::ResolveQueryData` writes each query with the
//! width its `D3D12_QUERY_TYPE` defines and takes no stride parameter — unlike
//! `vkCmdCopyQueryPoolResults`, which is handed one. A timestamp and an
//! occlusion count are each a `UINT64`, which is what
//! [`Device::query_results`](crcbl_hal::Device::query_results) and
//! [`resolve_query_set`](crcbl_hal::CommandEncoder::resolve_query_set) both
//! assume; a pipeline-statistics query is a whole
//! `D3D12_QUERY_DATA_PIPELINE_STATISTICS`, which is
//! [`PIPELINE_STATISTICS_COUNTERS`] of them.
//!
//! So [`result_bytes`] is the number every size, offset and bound in the query
//! path is derived from, and a caller that sized a destination for one `u64` per
//! query gets [`HalError::InvalidDescriptor`] from [`check_destination`] rather
//! than a resolve that writes past the end of its buffer.
//!
//! # Not Windows-only, and that is the point
//!
//! This module holds no `windows` type — it is `u64` arithmetic and one
//! [`HalError`] — so off Windows it exists in the test build alone and
//! `cargo test` on any host checks it, exactly as [`crate::sync`] and
//! [`crate::root`] are compiled for. That matters most for [`result_bytes`]:
//! `crate::conv` holds a `const` assertion that each figure equals the D3D12
//! structure's own `size_of`, so the literal here cannot drift from the ABI, and
//! everything derived from it is checked on a machine with no D3D12 at all.

use crcbl_hal::{HalError, QueryKind};

/// Counters `D3D12_QUERY_DATA_PIPELINE_STATISTICS` holds.
///
/// Fixed by the D3D12 ABI — `IAVertices` through `CSInvocations` — rather than
/// chosen here, which is why it is a literal at all. `crate::conv` holds the
/// compile-time assertion that ties [`result_bytes`] to the bindings' own
/// structure, which is what stops this drifting on a `windows` upgrade.
pub(crate) const PIPELINE_STATISTICS_COUNTERS: u64 = 11;

/// Alignment `ResolveQueryData` requires of its
/// `AlignedDestinationBufferOffset`.
///
/// D3D12's own rule, and the reason the parameter is named for it. Checked
/// rather than assumed because a caller chooses the offset:
/// [`resolve_query_set`](crcbl_hal::CommandEncoder::resolve_query_set) takes a
/// `dst_offset` from above the seam, where nothing knows this exists.
pub(crate) const RESOLVE_OFFSET_ALIGNMENT: u64 = 8;

/// Bytes one resolved query of this kind occupies.
///
/// The stride `ResolveQueryData` writes with, and so the size of a query set's
/// resolve destination, the offset of a query within one, and the bound a
/// caller's buffer is checked against. See the module docs for why D3D12 gets
/// to choose it.
pub(crate) const fn result_bytes(kind: QueryKind) -> u64 {
    match kind {
        // `D3D12_QUERY_TYPE_TIMESTAMP` resolves one `UINT64` tick and
        // `D3D12_QUERY_TYPE_OCCLUSION` one `UINT64` sample count, which is the
        // one `u64` per query the seam's two read paths are shaped for.
        QueryKind::Timestamp | QueryKind::Occlusion => size_of::<u64>() as u64,
        QueryKind::PipelineStatistics => PIPELINE_STATISTICS_COUNTERS * size_of::<u64>() as u64,
    }
}

/// The bytes a set of `count` queries resolves into, or why there is no such
/// set.
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`] for a count of zero, which is a query heap
/// D3D12 will not create and a set nothing could be written into.
pub(crate) fn resolve_buffer_bytes(kind: QueryKind, count: u32) -> Result<u64, HalError> {
    if count == 0 {
        return Err(HalError::InvalidDescriptor(
            "QuerySetDesc::count must be non-zero".to_string(),
        ));
    }
    Ok(u64::from(count) * result_bytes(kind))
}

/// Checks a read or a resolve against the set it names.
///
/// The bound [`Device::query_results`](crcbl_hal::Device::query_results)
/// documents — `first_query + queries` may not exceed the set — expressed in
/// `u64` so that a range near [`u32::MAX`] cannot wrap into a range that passes.
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`] naming both ends and the set's size.
pub(crate) fn check_range(count: u32, first_query: u32, queries: u64) -> Result<(), HalError> {
    let end = u64::from(first_query) + queries;
    if end > u64::from(count) {
        return Err(HalError::InvalidDescriptor(format!(
            "query range {first_query}..{end} exceeds the set's {count} queries"
        )));
    }
    Ok(())
}

/// Bytes `queries` queries of this kind occupy, back to back.
///
/// Both the length of a resolve and the offset of query `n` — which is
/// `span_bytes(kind, n)`, and is why every stride is itself a legal
/// [`RESOLVE_OFFSET_ALIGNMENT`].
///
/// No overflow check, and the caller is what rules one out: every count reaching
/// here has been through [`check_range`] against a `u32` query count, so the
/// widest product is `u32::MAX` strides and fits a `u64` with room to spare.
pub(crate) const fn span_bytes(kind: QueryKind, queries: u64) -> u64 {
    queries * result_bytes(kind)
}

/// Checks a caller's resolve destination against D3D12's rules and its own size.
///
/// Both halves matter and neither is D3D12's to report: an unaligned offset is
/// undefined behaviour the runtime does not diagnose, and a destination sized
/// for one `u64` per query is a **buffer overrun** on a pipeline-statistics set,
/// which writes [`PIPELINE_STATISTICS_COUNTERS`] of them per query.
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`] for an offset that is not a multiple of
/// [`RESOLVE_OFFSET_ALIGNMENT`], for a span that overflows, or for one that runs
/// past `dst_size`.
///
/// `queries` must already have been through [`check_range`]; see
/// [`span_bytes`].
pub(crate) fn check_destination(
    kind: QueryKind,
    dst_offset: u64,
    queries: u64,
    dst_size: u64,
) -> Result<(), HalError> {
    if !dst_offset.is_multiple_of(RESOLVE_OFFSET_ALIGNMENT) {
        return Err(HalError::InvalidDescriptor(format!(
            "resolve_query_set's dst_offset is {dst_offset}, and ResolveQueryData requires an \
             offset that is a multiple of {RESOLVE_OFFSET_ALIGNMENT}"
        )));
    }
    let end = dst_offset
        .checked_add(span_bytes(kind, queries))
        .ok_or_else(|| {
            HalError::InvalidDescriptor(format!(
                "resolving {queries} {kind:?} queries at offset {dst_offset} overflows a u64"
            ))
        })?;
    if end > dst_size {
        return Err(HalError::InvalidDescriptor(format!(
            "resolving {queries} {kind:?} queries writes {dst_offset}..{end} of a {dst_size}-byte \
             buffer; D3D12 resolves {} bytes per query and takes no stride to narrow that with",
            result_bytes(kind)
        )));
    }
    Ok(())
}

/// Nanoseconds one timestamp tick is worth, from
/// `ID3D12CommandQueue::GetTimestampFrequency`.
///
/// The reciprocal in nanoseconds, which is
/// [`Limits::timestamp_period_ns`](crcbl_hal::Limits::timestamp_period_ns)'s
/// definition and what `wgpu-hal`'s `dx12` backend computes in
/// `Queue::get_timestamp_period`.
///
/// # Errors
///
/// [`HalError::Backend`] for a frequency of zero. A tick worth infinitely many
/// nanoseconds is not a period, and the seam reserves `0.0` for a device without
/// [`Features::TIMESTAMP_QUERY`](crcbl_hal::Features::TIMESTAMP_QUERY) — so
/// there is no value to report here that would not be read as one of those two
/// claims. A queue that answered zero is broken, and saying so at device open is
/// the only place a caller can act on it.
pub(crate) fn timestamp_period_ns(frequency: u64) -> Result<f32, HalError> {
    /// Nanoseconds in the second `GetTimestampFrequency` counts ticks per.
    const NANOS_PER_SECOND: f64 = 1_000_000_000.0;

    if frequency == 0 {
        return Err(HalError::Backend(
            "ID3D12CommandQueue::GetTimestampFrequency reported zero ticks per second, so a \
             timestamp delta has no duration to convert to"
                .to_string(),
        ));
    }
    // A frequency is a tick rate in the low billions, so the `u64` -> `f64`
    // conversion is exact, and the period is a small positive number the `f32`
    // the seam carries holds to more digits than any clock is accurate to.
    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
    Ok((NANOS_PER_SECOND / frequency as f64) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A timestamp and an occlusion count are one `u64`; a statistics query is
    /// not.** That difference is what every bound in this module exists for.
    ///
    /// Red if `PipelineStatistics` falls back to eight bytes, which is the
    /// mistake that reads correctly: every size would look right, and a resolve
    /// of two statistics queries would write eighty-eight bytes past the end of
    /// a sixteen-byte destination with nothing anywhere reporting it.
    #[test]
    fn a_statistics_query_is_wider_than_the_seams_one_u64_per_query() {
        assert_eq!(result_bytes(QueryKind::Timestamp), size_of::<u64>() as u64);
        assert_eq!(result_bytes(QueryKind::Occlusion), size_of::<u64>() as u64);
        assert_eq!(
            result_bytes(QueryKind::PipelineStatistics),
            PIPELINE_STATISTICS_COUNTERS * size_of::<u64>() as u64
        );
        assert!(
            result_bytes(QueryKind::PipelineStatistics) > result_bytes(QueryKind::Timestamp),
            "a statistics query that fitted in one u64 would make every destination check vacuous"
        );
        // Every stride is a legal `AlignedDestinationBufferOffset`, which is what
        // makes the offset of query `n` legal for every `n`.
        for kind in [
            QueryKind::Timestamp,
            QueryKind::Occlusion,
            QueryKind::PipelineStatistics,
        ] {
            assert!(
                result_bytes(kind).is_multiple_of(RESOLVE_OFFSET_ALIGNMENT),
                "{kind:?} resolves at a stride ResolveQueryData could not be given as an offset"
            );
        }
    }

    /// A set of no queries is refused, and a set of some sizes to its stride.
    #[test]
    fn a_query_set_of_zero_queries_has_no_resolve_buffer() {
        assert_eq!(
            resolve_buffer_bytes(QueryKind::Timestamp, 4).expect("four timestamps"),
            4 * size_of::<u64>() as u64
        );
        assert_eq!(
            resolve_buffer_bytes(QueryKind::PipelineStatistics, 2).expect("two statistics"),
            2 * PIPELINE_STATISTICS_COUNTERS * size_of::<u64>() as u64
        );
        let error = resolve_buffer_bytes(QueryKind::Occlusion, 0).expect_err("no queries");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
    }

    /// **The bound is `first + len <= count`, and it is evaluated in `u64`.**
    ///
    /// Red if the addition is done in `u32`: `check_range(2, u32::MAX, 2)` wraps
    /// to `1`, which is inside a two-query set, so a read one past the end — the
    /// pair the seam suite checks — would be accepted on exactly the input a
    /// caller is most likely to have arrived at by arithmetic.
    #[test]
    fn a_read_past_the_end_of_a_set_is_refused_and_the_bound_cannot_wrap() {
        check_range(2, 0, 2).expect("the whole set");
        check_range(2, 1, 1).expect("the last query");
        check_range(2, 2, 0).expect("an empty read at the end is inside the set");

        let error = check_range(2, 0, 3).expect_err("one past the end");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
        let text = error.to_string();
        assert!(text.contains('3') && text.contains('2'), "{text}");

        assert!(
            check_range(2, u32::MAX, 2).is_err(),
            "the range wrapped and a read far past the end was accepted"
        );
    }

    /// The destination's two failures: an offset D3D12 will not take, and a
    /// buffer the resolve would run off the end of.
    ///
    /// Red if either check is dropped. Without the alignment check a caller's
    /// four-byte offset reaches `ResolveQueryData` as undefined behaviour the
    /// runtime does not report; without the size check a statistics resolve
    /// overruns a destination sized for timestamps.
    #[test]
    fn a_resolve_destination_must_be_aligned_and_large_enough() {
        check_destination(QueryKind::Timestamp, 0, 2, 16).expect("exactly two timestamps");
        check_destination(QueryKind::Timestamp, 8, 1, 16).expect("the second slot");

        let error = check_destination(QueryKind::Timestamp, 4, 1, 64).expect_err("unaligned");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
        assert!(error.to_string().contains('4'), "{error}");

        let error = check_destination(QueryKind::Timestamp, 8, 2, 16).expect_err("one too many");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

        // The case the whole check exists for: a destination sized at the seam's
        // one `u64` per query, handed a statistics set.
        let error = check_destination(QueryKind::PipelineStatistics, 0, 2, 16)
            .expect_err("statistics resolve eleven u64s each");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
        check_destination(
            QueryKind::PipelineStatistics,
            0,
            2,
            2 * PIPELINE_STATISTICS_COUNTERS * size_of::<u64>() as u64,
        )
        .expect("a destination sized for what D3D12 actually writes");

        // Overflow is refused rather than wrapping into a range that fits.
        assert!(check_destination(QueryKind::Timestamp, u64::MAX - 7, 2, u64::MAX).is_err());
    }

    /// The period is the reciprocal in nanoseconds, and zero is refused.
    ///
    /// Red if the division is inverted: a frequency of 1 GHz would report a
    /// billion nanoseconds per tick rather than one, and every duration derived
    /// from a delta would be out by eighteen orders of magnitude while still
    /// being a positive number the seam suite accepts.
    #[test]
    fn a_timestamp_period_is_nanoseconds_per_tick_and_never_zero() {
        assert!(
            (timestamp_period_ns(1_000_000_000).expect("a 1 GHz counter") - 1.0).abs()
                < f32::EPSILON
        );
        assert!(
            (timestamp_period_ns(100_000_000).expect("a 100 MHz counter") - 10.0).abs()
                < f32::EPSILON
        );
        // A slower counter has a longer tick, which is the direction the
        // inverted division gets backwards.
        assert!(
            timestamp_period_ns(1_000_000).expect("1 MHz")
                > timestamp_period_ns(1_000_000_000).expect("1 GHz")
        );

        let error = timestamp_period_ns(0).expect_err("a stopped clock has no period");
        assert!(matches!(error, HalError::Backend(_)), "{error:?}");
    }
}
