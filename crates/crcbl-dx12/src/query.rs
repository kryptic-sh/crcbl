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
//! [`PIPELINE_STATISTICS_COUNTERS`] of them — or, on a device that counts the
//! mesh stages, a `D3D12_QUERY_DATA_PIPELINE_STATISTICS1`, which is
//! [`PIPELINE_STATISTICS1_COUNTERS`].
//!
//! So a set's [`QueryLayout`] is what every size, offset and bound in the query
//! path is derived from, and a caller that sized a destination for one `u64` per
//! query gets [`HalError::InvalidDescriptor`] from [`check_destination`] rather
//! than a resolve that writes past the end of its buffer.
//!
//! # Not Windows-only, and that is the point
//!
//! This module holds no `windows` type — it is `u64` arithmetic and one
//! [`HalError`] — so off Windows it exists in the test build alone and
//! `cargo test` on any host checks it, exactly as [`crate::sync`] and
//! [`crate::root`] are compiled for. That matters most for
//! [`QueryLayout::result_bytes`]: `crate::conv` holds a `const` assertion that
//! each figure equals the D3D12
//! structure's own `size_of`, so the literal here cannot drift from the ABI, and
//! everything derived from it is checked on a machine with no D3D12 at all.

use std::num::NonZeroU64;

use crcbl_hal::{HalError, PipelineStatistics, QueryKind};

/// Counters `D3D12_QUERY_DATA_PIPELINE_STATISTICS` holds.
///
/// Fixed by the D3D12 ABI — `IAVertices` through `CSInvocations` — rather than
/// chosen here, which is why it is a literal at all. `crate::conv` holds the
/// compile-time assertion that ties [`QueryLayout::result_bytes`] to the
/// bindings' own structure, which is what stops this drifting on a `windows`
/// upgrade.
pub(crate) const PIPELINE_STATISTICS_COUNTERS: u64 = 11;

/// Counters `D3D12_QUERY_DATA_PIPELINE_STATISTICS1` holds: the plain
/// structure's, then `ASInvocations`, `MSInvocations` and `MSPrimitives`.
///
/// Fixed by the ABI, and asserted against it in `crate::conv`, for
/// [`PIPELINE_STATISTICS_COUNTERS`]' reasons.
pub(crate) const PIPELINE_STATISTICS1_COUNTERS: u64 = 14;

/// Alignment `ResolveQueryData` requires of its
/// `AlignedDestinationBufferOffset`.
///
/// D3D12's own rule, and the reason the parameter is named for it. Checked
/// rather than assumed because a caller chooses the offset:
/// [`resolve_query_set`](crcbl_hal::CommandEncoder::resolve_query_set) takes a
/// `dst_offset` from above the seam, where nothing knows this exists.
pub(crate) const RESOLVE_OFFSET_ALIGNMENT: u64 = 8;

/// What one query of a set resolves as: the set's kind, and for a statistics
/// set which of D3D12's two statistics structures its heap was created for.
///
/// **The kind alone does not decide the width.** A device that reports
/// `MeshShaderPipelineStatsSupported` gets `_PIPELINE_STATISTICS1` heaps, whose
/// structure is the plain one with the amplification and mesh counters
/// appended, so a set carries its layout from creation and every size here is
/// asked of that.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QueryLayout {
    /// One `UINT64` tick.
    Timestamp,
    /// One `UINT64` sample count.
    Occlusion,
    /// `D3D12_QUERY_DATA_PIPELINE_STATISTICS`.
    Statistics,
    /// `D3D12_QUERY_DATA_PIPELINE_STATISTICS1`.
    StatisticsWithMesh,
}

impl QueryLayout {
    /// The layout a set of `kind` gets on a device that does, or does not,
    /// count the mesh stages.
    pub(crate) const fn of(kind: QueryKind, mesh_statistics: bool) -> Self {
        match kind {
            QueryKind::Timestamp => Self::Timestamp,
            QueryKind::Occlusion => Self::Occlusion,
            QueryKind::PipelineStatistics if mesh_statistics => Self::StatisticsWithMesh,
            QueryKind::PipelineStatistics => Self::Statistics,
        }
    }

    /// The `UINT64` counters one query resolves.
    pub(crate) const fn counters(self) -> u64 {
        match self {
            // The one `u64` per query the seam's two read paths are shaped for.
            Self::Timestamp | Self::Occlusion => 1,
            Self::Statistics => PIPELINE_STATISTICS_COUNTERS,
            Self::StatisticsWithMesh => PIPELINE_STATISTICS1_COUNTERS,
        }
    }

    /// Bytes one resolved query occupies.
    ///
    /// The stride `ResolveQueryData` writes with, and so the size of a query
    /// set's resolve destination, the offset of a query within one, and the
    /// bound a caller's buffer is checked against. See the module docs for why
    /// D3D12 gets to choose it.
    pub(crate) const fn result_bytes(self) -> u64 {
        self.counters() * size_of::<u64>() as u64
    }
}

/// The bytes a set of `count` queries resolves into, or why there is no such
/// set.
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`] for a count of zero, which is a query heap
/// D3D12 will not create and a set nothing could be written into.
pub(crate) fn resolve_buffer_bytes(layout: QueryLayout, count: u32) -> Result<u64, HalError> {
    if count == 0 {
        return Err(HalError::InvalidDescriptor(
            "QuerySetDesc::count must be non-zero".to_string(),
        ));
    }
    Ok(u64::from(count) * layout.result_bytes())
}

/// One resolved statistics query, as the seam's counts.
///
/// `words` is the query's [`QueryLayout::counters`] values in the structure's
/// own order — `IAVertices`, `IAPrimitives`, `VSInvocations`, `GSInvocations`,
/// `GSPrimitives`, `CInvocations`, `CPrimitives`, `PSInvocations`,
/// `HSInvocations`, `DSInvocations`, `CSInvocations`, then for
/// `_STATISTICS1` `ASInvocations`, `MSInvocations` and `MSPrimitives`. The
/// geometry and tessellation counters have no seam field and are dropped.
///
/// **Every stage the structure has is `Some`**, a mesh draw's pass included:
/// unlike Vulkan, D3D12 counts a mixed pass in one query, so a zero
/// input-assembly count beside a mesh draw is the whole answer rather than half
/// of one. The mesh fields are `None` only for the plain structure, which has
/// no such counters.
///
/// # Panics
///
/// If `words` is shorter than the layout's counters, which is a caller that
/// sliced the readback wrongly.
pub(crate) fn decode_statistics(layout: QueryLayout, words: &[u64]) -> PipelineStatistics {
    let mesh = matches!(layout, QueryLayout::StatisticsWithMesh);
    let at = |index: usize| Some(words[index]);
    let mesh_at = |index: usize| mesh.then(|| words[index]);
    PipelineStatistics {
        input_vertices: at(0),
        input_primitives: at(1),
        vertex_invocations: at(2),
        clipper_invocations: at(5),
        clipper_primitives: at(6),
        fragment_invocations: at(7),
        compute_invocations: at(10),
        task_invocations: mesh_at(11),
        mesh_invocations: mesh_at(12),
        mesh_primitives: mesh_at(13),
    }
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

/// Bytes `queries` queries of this layout occupy, back to back.
///
/// Both the length of a resolve and the offset of query `n` — which is
/// `span_bytes(layout, n)`, and is why every stride is itself a legal
/// [`RESOLVE_OFFSET_ALIGNMENT`].
///
/// No overflow check, and the caller is what rules one out: every count reaching
/// here has been through [`check_range`] against a `u32` query count, so the
/// widest product is `u32::MAX` strides and fits a `u64` with room to spare.
pub(crate) const fn span_bytes(layout: QueryLayout, queries: u64) -> u64 {
    queries * layout.result_bytes()
}

/// Checks a caller's resolve destination against D3D12's rules and its own size.
///
/// Both halves matter and neither is D3D12's to report: an unaligned offset is
/// undefined behaviour the runtime does not diagnose, and a destination sized
/// for one `u64` per query is a **buffer overrun** on a pipeline-statistics set,
/// which writes [`QueryLayout::counters`] of them per query.
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
    layout: QueryLayout,
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
        .checked_add(span_bytes(layout, queries))
        .ok_or_else(|| {
            HalError::InvalidDescriptor(format!(
                "resolving {queries} {layout:?} queries at offset {dst_offset} overflows a u64"
            ))
        })?;
    if end > dst_size {
        return Err(HalError::InvalidDescriptor(format!(
            "resolving {queries} {layout:?} queries writes {dst_offset}..{end} of a \
             {dst_size}-byte buffer; D3D12 resolves {} bytes per query and takes no stride to \
             narrow that with",
            layout.result_bytes()
        )));
    }
    Ok(())
}

/// Nanoseconds in the second `ID3D12CommandQueue::GetTimestampFrequency` counts
/// ticks per.
const NANOS_PER_SECOND: u128 = 1_000_000_000;

/// The timestamp clock's rate, checked to be a rate.
///
/// # Errors
///
/// [`HalError::Backend`] for a frequency of zero. A clock that ticks zero times
/// a second has no duration to convert a tick to, and every timestamp read
/// through it would be a division by zero. A queue that answered zero is broken,
/// and device open is the only place a caller can act on it — which is why this
/// returns a [`NonZeroU64`] rather than leaving the check to be repeated at
/// every read.
pub(crate) fn timestamp_frequency(frequency: u64) -> Result<NonZeroU64, HalError> {
    NonZeroU64::new(frequency).ok_or_else(|| {
        HalError::Backend(
            "ID3D12CommandQueue::GetTimestampFrequency reported zero ticks per second, so a \
             timestamp delta has no duration to convert to"
                .to_string(),
        )
    })
}

/// A raw D3D12 timestamp in nanoseconds, which is what
/// [`Device::query_results`](crcbl_hal::Device::query_results) reports.
///
/// D3D12 states its clock as a *rate* — `GetTimestampFrequency` answers ticks
/// per second — where Vulkan states the reciprocal, so this divides where
/// `crcbl_vk::conv::timestamp_nanos` multiplies. That is the whole reason the
/// seam carries neither number: they are two APIs' spellings of a thing Metal
/// and WebGPU do not express at all.
///
/// # Exact, because the frequency makes it exact
///
/// `ticks * 1e9 / frequency` is a rational with an integer numerator and an
/// integer denominator, so there is no float anywhere in it: the product is
/// taken in `u128`, which holds a full `u64` tick count times a billion with
/// room to spare, and the division rounds to nearest by adding half the divisor
/// first. Nothing here quantises the way a `f64` multiply would once a
/// free-running counter passes 2⁵³.
///
/// Rounding is to nearest, applied once here rather than by each caller: the
/// seam's contract is a nanosecond count, so the fraction has to go somewhere,
/// and one rule at the source is the only way two readers agree.
pub(crate) fn timestamp_nanos(ticks: u64, frequency: NonZeroU64) -> u64 {
    let divisor = u128::from(frequency.get());
    let nanos = (u128::from(ticks) * NANOS_PER_SECOND + divisor / 2) / divisor;
    // Saturating rather than wrapping: a reading so large it leaves `u64` is a
    // broken clock, and `u64::MAX` says "past anything" where a wrap would say
    // "just after boot".
    u64::try_from(nanos).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A timestamp and an occlusion count are one `u64`; a statistics query is
    /// not, and the mesh-counting one is wider still.** That difference is what
    /// every bound in this module exists for.
    ///
    /// Red if `Statistics` falls back to eight bytes, which is the mistake that
    /// reads correctly: every size would look right, and a resolve of two
    /// statistics queries would write eighty-eight bytes past the end of a
    /// sixteen-byte destination with nothing anywhere reporting it. Red too if
    /// [`QueryLayout::of`] ignores the device's answer, which sizes a
    /// `_STATISTICS1` heap's buffer for the plain structure and loses the last
    /// query's mesh counters off its end.
    #[test]
    fn a_statistics_query_is_wider_than_the_seams_one_u64_per_query() {
        let eight = size_of::<u64>() as u64;
        assert_eq!(QueryLayout::Timestamp.result_bytes(), eight);
        assert_eq!(QueryLayout::Occlusion.result_bytes(), eight);
        assert_eq!(
            QueryLayout::Statistics.result_bytes(),
            PIPELINE_STATISTICS_COUNTERS * eight
        );
        assert_eq!(
            QueryLayout::StatisticsWithMesh.result_bytes(),
            PIPELINE_STATISTICS1_COUNTERS * eight
        );
        assert!(
            QueryLayout::Statistics.result_bytes() > QueryLayout::Timestamp.result_bytes(),
            "a statistics query that fitted in one u64 would make every destination check vacuous"
        );

        let kind = QueryKind::PipelineStatistics;
        assert_eq!(QueryLayout::of(kind, false), QueryLayout::Statistics);
        assert_eq!(QueryLayout::of(kind, true), QueryLayout::StatisticsWithMesh);
        // Mesh counting is a statistics matter; a timestamp is one tick either way.
        for mesh in [false, true] {
            assert_eq!(
                QueryLayout::of(QueryKind::Timestamp, mesh),
                QueryLayout::Timestamp
            );
            assert_eq!(
                QueryLayout::of(QueryKind::Occlusion, mesh),
                QueryLayout::Occlusion
            );
        }

        // Every stride is a legal `AlignedDestinationBufferOffset`, which is what
        // makes the offset of query `n` legal for every `n`.
        for layout in [
            QueryLayout::Timestamp,
            QueryLayout::Occlusion,
            QueryLayout::Statistics,
            QueryLayout::StatisticsWithMesh,
        ] {
            assert!(
                layout
                    .result_bytes()
                    .is_multiple_of(RESOLVE_OFFSET_ALIGNMENT),
                "{layout:?} resolves at a stride ResolveQueryData could not be given as an offset"
            );
        }
    }

    /// **Each counter lands in its own field**, and the mesh fields are `None`
    /// exactly when the structure has no such counters.
    ///
    /// Every word is distinct, so a field read from its neighbour's slot — the
    /// geometry-shader pair sits between the vertex and clipper counters, and
    /// skipping it is the mistake an index off by two makes — reads a value
    /// this asserts against.
    #[test]
    fn a_resolved_statistics_query_decodes_counter_by_counter() {
        let words: Vec<u64> = (100..100 + PIPELINE_STATISTICS1_COUNTERS).collect();
        let with_mesh = decode_statistics(QueryLayout::StatisticsWithMesh, &words);
        assert_eq!(
            with_mesh,
            PipelineStatistics {
                input_vertices: Some(100),
                input_primitives: Some(101),
                vertex_invocations: Some(102),
                clipper_invocations: Some(105),
                clipper_primitives: Some(106),
                fragment_invocations: Some(107),
                compute_invocations: Some(110),
                task_invocations: Some(111),
                mesh_invocations: Some(112),
                mesh_primitives: Some(113),
            }
        );

        let plain = decode_statistics(
            QueryLayout::Statistics,
            &words[..PIPELINE_STATISTICS_COUNTERS as usize],
        );
        assert_eq!(
            plain,
            PipelineStatistics {
                task_invocations: None,
                mesh_invocations: None,
                mesh_primitives: None,
                ..with_mesh
            },
            "the plain structure has no mesh counters to report"
        );
    }

    /// A set of no queries is refused, and a set of some sizes to its stride.
    ///
    /// The occlusion kind is what the zero-count half asks about deliberately:
    /// `Device::create_query_set` refuses that kind before it reaches here (see
    /// `crcbl_hal::NO_OCCLUSION_QUERY_VERB`), so this is the one place the
    /// count rule is still asked of it, and the answer must not depend on which
    /// kind was named.
    #[test]
    fn a_query_set_of_zero_queries_has_no_resolve_buffer() {
        assert_eq!(
            resolve_buffer_bytes(QueryLayout::Timestamp, 4).expect("four timestamps"),
            4 * size_of::<u64>() as u64
        );
        assert_eq!(
            resolve_buffer_bytes(QueryLayout::Statistics, 2).expect("two statistics"),
            2 * PIPELINE_STATISTICS_COUNTERS * size_of::<u64>() as u64
        );
        let error = resolve_buffer_bytes(QueryLayout::Occlusion, 0).expect_err("no queries");
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
        check_destination(QueryLayout::Timestamp, 0, 2, 16).expect("exactly two timestamps");
        check_destination(QueryLayout::Timestamp, 8, 1, 16).expect("the second slot");

        let error = check_destination(QueryLayout::Timestamp, 4, 1, 64).expect_err("unaligned");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
        assert!(error.to_string().contains('4'), "{error}");

        let error = check_destination(QueryLayout::Timestamp, 8, 2, 16).expect_err("one too many");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

        // The case the whole check exists for: a destination sized at the seam's
        // one `u64` per query, handed a statistics set.
        let error = check_destination(QueryLayout::Statistics, 0, 2, 16)
            .expect_err("statistics resolve eleven u64s each");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
        check_destination(
            QueryLayout::Statistics,
            0,
            2,
            2 * PIPELINE_STATISTICS_COUNTERS * size_of::<u64>() as u64,
        )
        .expect("a destination sized for what D3D12 actually writes");

        // Overflow is refused rather than wrapping into a range that fits.
        assert!(check_destination(QueryLayout::Timestamp, u64::MAX - 7, 2, u64::MAX).is_err());
    }

    /// A stopped clock is refused at device open rather than divided by.
    #[test]
    fn a_timestamp_frequency_of_zero_is_refused() {
        assert_eq!(
            timestamp_frequency(1_000_000_000)
                .expect("a 1 GHz counter")
                .get(),
            1_000_000_000
        );
        let error = timestamp_frequency(0).expect_err("a stopped clock has no rate");
        assert!(matches!(error, HalError::Backend(_)), "{error:?}");
    }

    /// **The seam reports nanoseconds, so the frequency has to be spent here.**
    ///
    /// Red if `timestamp_nanos` returns its ticks unscaled, which is the
    /// mistake that reads correctly on the 1 GHz counter where a tick happens
    /// to be a nanosecond and is out by a factor of ten on a 100 MHz one. Red
    /// too if the division is inverted — a 1 GHz counter would then report a
    /// billion nanoseconds per tick, out by eighteen orders of magnitude while
    /// still being a positive number every "it moved" assertion accepts.
    #[test]
    fn a_timestamp_is_scaled_by_the_queues_frequency() {
        let ghz = timestamp_frequency(1_000_000_000).expect("1 GHz");
        let hundred_mhz = timestamp_frequency(100_000_000).expect("100 MHz");
        assert_eq!(timestamp_nanos(744, ghz), 744);
        assert_eq!(timestamp_nanos(744, hundred_mhz), 7_440);
        assert_eq!(timestamp_nanos(0, hundred_mhz), 0);
        // A slower counter makes a longer tick, which is the direction an
        // inverted division gets backwards.
        assert!(timestamp_nanos(1, hundred_mhz) > timestamp_nanos(1, ghz));
    }

    /// The fraction rounds to nearest, and the whole `u64` tick range converts
    /// without the `f64` cliff.
    ///
    /// Red on truncation: a 3 GHz counter turns two ticks into `0` rather than
    /// `1`. Red on `(ticks as f64 * period).round() as u64`: past 2⁵³ an `f64`
    /// cannot hold consecutive integers, so a counter that has been running for
    /// months reports two readings as the same instant and a delta of a few
    /// hundred nanoseconds arrives as zero.
    #[test]
    fn a_timestamp_rounds_to_nearest_and_survives_a_long_running_counter() {
        let three_ghz = timestamp_frequency(3_000_000_000).expect("3 GHz");
        assert_eq!(timestamp_nanos(2, three_ghz), 1, "0.667 ns rounds to 1");
        assert_eq!(timestamp_nanos(1, three_ghz), 0, "0.333 ns rounds to 0");

        let ghz = timestamp_frequency(1_000_000_000).expect("1 GHz");
        let past_the_mantissa = (1u64 << 53) + 8;
        assert_eq!(timestamp_nanos(past_the_mantissa, ghz), past_the_mantissa);
        assert_eq!(
            timestamp_nanos(past_the_mantissa + 1, ghz) - timestamp_nanos(past_the_mantissa, ghz),
            1,
            "one tick of a 1 GHz clock is one nanosecond however long the GPU has been up"
        );
        // And a reading no clock could reach saturates rather than wrapping into
        // a plausible-looking instant just after boot.
        assert_eq!(
            timestamp_nanos(u64::MAX, timestamp_frequency(1_000).expect("1 kHz")),
            u64::MAX
        );
    }
}
